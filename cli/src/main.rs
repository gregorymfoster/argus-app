use argus_engine::compute::Compute;
use argus_engine::library::{Author, ImportOptions, Library, PhotoFilter, Verdict};
use argus_engine::recipe::Recipe;
use argus_engine::tools::Tools;
use argus_engine::{decode, develop, embedded_preview, jpeg_dimensions, DevelopOptions, RgbImage};
mod fit;
mod mcp;
mod serve;
mod speed;

use clap::{Parser, Subcommand};
use std::{
    io::BufWriter,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

#[derive(Parser)]
#[command(
    name = "argus",
    about = "Argus engine CLI: render RAWs and drive a Library headlessly"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Decode + develop a RAW to JPEG (q90) or PNG, chosen by the --out extension. Prints one line of JSON timings.
    Render {
        file: PathBuf,
        /// Output path; .jpg/.jpeg -> JPEG q90, .png -> PNG. Default: out/<stem>.jpg
        #[arg(long)]
        out: Option<PathBuf>,
        /// Downsize so the longest edge is at most N pixels.
        #[arg(long)]
        long_edge: Option<usize>,
        /// Recipe JSON file to apply (see engine/src/recipe.rs).
        #[arg(long)]
        recipe: Option<PathBuf>,
        /// Settings on top of --recipe, e.g. `--set exposure=0.3,temp=-10,hsl.blue.sat=-20`.
        #[arg(long, value_delimiter = ',')]
        set: Vec<String>,
    },
    /// Change edit settings on Photos (one undoable batch), e.g.
    /// `argus edit 3 4 --library L exposure=0.3 temp=-10 hsl.blue.sat=-20`.
    Edit {
        #[arg(long)]
        library: PathBuf,
        /// Photo ids, then setting=value pairs.
        #[arg(required = true, num_args = 1..)]
        args: Vec<String>,
        /// Add the values to the current ones instead of replacing them.
        #[arg(long)]
        relative: bool,
    },
    /// Export Photos into a folder (never overwrites): JPEG or TIFF with their edits, or
    /// `--format original` for the untouched original plus an XMP sidecar for Lightroom.
    Export {
        #[arg(required = true)]
        ids: Vec<i64>,
        #[arg(long)]
        library: PathBuf,
        #[arg(long)]
        to: PathBuf,
        /// jpeg (default), tiff or original
        #[arg(long)]
        format: Option<String>,
        #[arg(long)]
        long_edge: Option<usize>,
        #[arg(long)]
        quality: Option<u8>,
        /// shoot (default: 2026-10-18_Boat-Trip_DSC08005), sequence or original
        #[arg(long)]
        naming: Option<String>,
    },
    /// Call any tool-layer tool (as the Agent and MCP do) with JSON arguments. Prints JSON
    /// parts; JPEG parts are written to --out (numbered when there are several).
    /// e.g. `argus tool render_photo '{"photo_id":3}' --library L --out out/p3.jpg`
    Tool {
        name: String,
        #[arg(default_value = "{}")]
        args: String,
        #[arg(long)]
        library: PathBuf,
        #[arg(long, default_value = "out/tool.jpg")]
        out: PathBuf,
    },
    /// Extract the camera's largest embedded JPEG unchanged.
    Preview {
        file: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Dev tool: fit the default Look to the camera's embedded JPEGs (see cli/src/fit.rs).
    FitLook {
        #[arg(long, default_value = "fixtures/raw")]
        raw_dir: PathBuf,
        #[arg(long, default_value = "out/fit")]
        out_dir: PathBuf,
        /// Skip the search and just evaluate: exposure_ev,shoulder_knee,shadow_lift,contrast,saturation
        #[arg(long, value_delimiter = ',')]
        eval: Option<Vec<f32>>,
    },
    /// Import a folder (recursively) into a Library as one new Shoot. Originals are only read.
    Import {
        folder: PathBuf,
        /// Library folder (created if missing).
        #[arg(long)]
        library: PathBuf,
        /// Shoot name; defaults to the folder's name.
        #[arg(long)]
        shoot: Option<String>,
        /// Preview size; defaults to the Library's Settings.
        #[arg(long)]
        preview_long_edge: Option<usize>,
        /// Copy the originals into the Library (the default for a memory Card).
        #[arg(long)]
        copy: bool,
        /// Read the originals where they are, even from a Card.
        #[arg(long, conflicts_with = "copy")]
        in_place: bool,
        /// Photos decoded at once (caps peak memory; each still develops on every core).
        #[arg(long, default_value_t = 4)]
        max_in_flight: usize,
        /// For RAW+JPEG frames, skip the camera's JPEG (by default it's kept with the RAW).
        #[arg(long)]
        raw_only: bool,
        /// Stop once RAWs show the camera's embedded preview, leaving Argus's own render to
        /// later (`argus previews`). By default Import renders them before it returns.
        #[arg(long)]
        camera_previews: bool,
        /// CPU share for rendering previews: gentle, balanced or full.
        /// Default: $ARGUS_COMPUTE or gentle.
        #[arg(long)]
        compute: Option<Compute>,
    },
    /// Time decode, Import, previews and Edit re-renders on sample RAWs (per Photo, plus an
    /// estimate for a 266-photo card) and compare with bench/speed-baseline.json. Exits 1
    /// on a regression. `just speed` runs it on fixtures/raw and fixtures/raw-a7rv.
    Speed {
        /// Folders of sample .ARW files.
        folders: Vec<PathBuf>,
        #[arg(long, default_value = "bench/speed-baseline.json")]
        baseline: PathBuf,
        /// Record these numbers as this machine's baseline.
        #[arg(long)]
        save: bool,
    },
    /// Time AI Denoise on a RAW without a Library: denoise a centre crop of about `--tiles`
    /// model tiles at full resolution, print seconds per tile and the projected time for the
    /// whole frame as JSON, and write the crop before, at Amount 50 and at 100 to `--out`. `--tiles 0`
    /// denoises the whole frame.
    Denoise {
        file: PathBuf,
        #[arg(long, default_value_t = 48)]
        tiles: usize,
        /// Where the model runs: auto (Core ML on the Mac, else the CPU), cpu or coreml.
        #[arg(long, default_value = "auto")]
        backend: String,
        /// Also run the crop on the CPU, the reference, and report its time and how far the
        /// two results differ (checks the Core ML model on a Mac).
        #[arg(long)]
        compare: bool,
        /// CPU share: gentle, balanced or full. Default: $ARGUS_COMPUTE or gentle.
        #[arg(long)]
        compute: Option<Compute>,
        #[arg(long, default_value = "out/denoise")]
        out: PathBuf,
    },
    /// Replace camera previews with Argus's own render (what Import leaves with
    /// --camera-previews, or the app builds in the background). Prints JSON.
    Previews {
        #[arg(long)]
        library: PathBuf,
        #[arg(long)]
        shoot: Option<i64>,
        /// CPU share: gentle, balanced or full. Default: $ARGUS_COMPUTE or gentle.
        #[arg(long)]
        compute: Option<Compute>,
    },
    /// List Shoots, or Photos (as JSON) when --shoot or --verdict is given or with --photos.
    List {
        #[arg(long)]
        library: PathBuf,
        #[arg(long)]
        shoot: Option<i64>,
        /// pick, reject or unflagged
        #[arg(long)]
        verdict: Option<Verdict>,
        #[arg(long)]
        photos: bool,
        /// Only Photos with at least this many stars.
        #[arg(long)]
        min_stars: Option<u8>,
        /// The Shoot's Best Photos, best first (needs --shoot, else the newest Shoot).
        #[arg(long)]
        best: bool,
    },
    /// AI cull a whole Shoot (Shoot context): Verdicts with reasons, star Ratings, Stacks and
    /// the Shoot's Best Photos. Person flags and stars are kept. Providers: ollama (default, local), claude-code (your Claude plan), claude-api.
    Cull {
        /// ollama | claude-code | claude-api (default: $ARGUS_PROVIDER or ollama)
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        library: PathBuf,
        /// Shoot id; defaults to the newest Shoot.
        #[arg(long)]
        shoot: Option<i64>,
        /// How much of the Mac a local model may take: gentle (default, for while you work),
        /// balanced, or full (overnight). Default: $ARGUS_COMPUTE or gentle.
        #[arg(long)]
        compute: Option<Compute>,
    },
    /// Ask the Agent to do something in plain words ("warmer, less contrast"); it edits
    /// through the tool layer as one undoable batch and prints what it did (JSON).
    Agent {
        prompt: String,
        #[arg(long)]
        library: PathBuf,
        /// The Photo in focus (as if open in Edit).
        #[arg(long)]
        photo: Option<i64>,
        /// Shoot id; defaults to the Photo's Shoot, else the newest.
        #[arg(long)]
        shoot: Option<i64>,
        /// ollama | claude-code | claude-api (default: $ARGUS_PROVIDER or ollama)
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        compute: Option<Compute>,
    },
    /// Work through the background queue (Suggestions, "apply to the rest", culls) by the
    /// app's rules: on battery, while the Mac runs hot or before overnight, held jobs wait.
    /// With --once, stop when nothing more may run now, else keep watching until stopped.
    Work {
        #[arg(long)]
        library: PathBuf,
        #[arg(long)]
        once: bool,
        /// Run every queued job now, ignoring the holds (benches and scripts).
        #[arg(long)]
        drain: bool,
        /// Override the queue's Compute level for this run.
        #[arg(long)]
        compute: Option<Compute>,
    },
    /// Back up the Library's database into its backups/ folder now (the app also makes one
    /// a day), or with --list show the backups there.
    Backup {
        #[arg(long)]
        library: PathBuf,
        #[arg(long)]
        list: bool,
    },
    /// Put a backup (a name from `argus backup --list`) back as the Library's database. The
    /// current state is backed up first, so this can be undone the same way.
    Restore {
        name: String,
        #[arg(long)]
        library: PathBuf,
    },
    /// Serve a Library over HTTP on 127.0.0.1 for the UI in a browser (dev bridge, ADR 0005).
    Serve {
        #[arg(long)]
        library: PathBuf,
        #[arg(long, default_value_t = 7878)]
        port: u16,
    },
    /// Serve the Library's tools over MCP on stdio, for Claude Desktop, Claude Code and other
    /// MCP clients (see docs/mcp.md).
    Mcp {
        #[arg(long)]
        library: PathBuf,
    },
    /// Set a Verdict (pick, reject, unflagged) on Photos by id, as the person (or --ai).
    Flag {
        verdict: Verdict,
        #[arg(required = true)]
        ids: Vec<i64>,
        #[arg(long)]
        library: PathBuf,
        #[arg(long)]
        reason: Option<String>,
        /// Record the change as the AI's, not the person's.
        #[arg(long)]
        ai: bool,
    },
    /// Set a star Rating (0-5, Lightroom stars; 0 clears it) on Photos.
    Rate {
        stars: u8,
        #[arg(required = true)]
        ids: Vec<i64>,
        #[arg(long)]
        library: PathBuf,
        /// Record the change as the AI's, not the person's.
        #[arg(long)]
        ai: bool,
    },
}

fn ms(t: Instant) -> f64 {
    (t.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.cmd {
        Cmd::Render {
            file,
            out,
            long_edge,
            recipe,
            set,
        } => {
            let mut r = match recipe {
                Some(p) => {
                    Recipe::from_value(&serde_json::from_str(&std::fs::read_to_string(p)?)?)?
                }
                None => Recipe::default(),
            };
            for kv in &set {
                let (k, v) = parse_setting(kv)?;
                r.set(k, v, false)?;
            }
            let out = out.unwrap_or_else(|| {
                PathBuf::from("out")
                    .join(file.file_stem().unwrap_or_default())
                    .with_extension("jpg")
            });
            if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            let t0 = Instant::now();
            let raw = decode(&file)?;
            let decode_ms = ms(t0);
            let t1 = Instant::now();
            let post = r.render_adjust();
            let mut img = develop(
                &raw,
                &DevelopOptions {
                    long_edge,
                    recipe: r,
                    ..Default::default()
                },
            );
            argus_engine::effects::post_crop(&mut img, &post);
            let develop_ms = ms(t1);
            let t2 = Instant::now();
            write_image(&out, &img)?;
            let encode_ms = ms(t2);
            println!(
                "{}",
                serde_line(
                    &file,
                    img.width,
                    img.height,
                    &[
                        ("decode_ms", decode_ms),
                        ("develop_ms", develop_ms),
                        ("encode_ms", encode_ms),
                        ("total_ms", ms(t0))
                    ]
                )
            );
            eprintln!(
                "camera: {} {}  sensor {}x{}  crop {:?}",
                raw.make, raw.model, raw.sensor_width, raw.sensor_height, raw.crop
            );
        }
        Cmd::Preview { file, out } => {
            let jpg = embedded_preview(&file)?;
            std::fs::write(&out, &jpg)?;
            let (w, h) = jpeg_dimensions(&jpg).unwrap_or((0, 0));
            println!(
                "{{\"file\":{:?},\"width\":{w},\"height\":{h},\"bytes\":{}}}",
                file.display().to_string(),
                jpg.len()
            );
        }
        Cmd::FitLook {
            raw_dir,
            out_dir,
            eval,
        } => {
            let look = eval.map(|v| {
                assert_eq!(v.len(), 5, "--eval needs 5 values");
                argus_engine::pipeline::Look {
                    exposure_ev: v[0],
                    shoulder_knee: v[1],
                    shadow_lift: v[2],
                    contrast: v[3],
                    saturation: v[4],
                    tone: argus_engine::pipeline::ToneSliders::NONE,
                }
            });
            fit::run(&raw_dir, &out_dir, look)?;
        }
        Cmd::Import {
            folder,
            library,
            shoot,
            preview_long_edge,
            copy,
            in_place,
            max_in_flight,
            raw_only,
            camera_previews,
            compute,
        } => {
            let mut lib = Library::open(&library)?;
            let report = lib.import(
                &folder,
                &ImportOptions {
                    shoot_name: shoot,
                    preview_long_edge,
                    max_in_flight,
                    files: None,
                    raw_only,
                    copy: copy || (!in_place && argus_engine::library::is_on_card(&folder)),
                    camera_previews: true,
                },
            )?;
            let mut out = serde_json::to_value(&report)?;
            if let (false, Some(id)) = (camera_previews, report.shoot_id) {
                let compute = compute.unwrap_or_else(Compute::from_env);
                let previews = lib.render_previews(Some(id), compute, &|_, _| {})?;
                out["previews"] = serde_json::to_value(previews)?;
            }
            println!("{out}");
        }
        Cmd::Speed {
            folders,
            baseline,
            save,
        } => {
            if !speed::run(&folders, &baseline, save)? {
                eprintln!("speed: slower than the baseline (see regressions)");
                std::process::exit(1);
            }
        }
        Cmd::Denoise {
            file,
            tiles,
            backend,
            compare,
            compute,
            out,
        } => denoise_time(
            &file,
            tiles,
            &backend,
            compare,
            compute.unwrap_or_else(Compute::from_env),
            &out,
        )?,
        Cmd::Previews {
            library,
            shoot,
            compute,
        } => {
            let lib = Library::open(&library)?;
            let compute = compute.unwrap_or_else(Compute::from_env);
            let report = lib.render_previews(shoot, compute, &|done, total| {
                eprintln!("previews {done}/{total}");
            })?;
            println!("{}", serde_json::to_string(&report)?);
        }
        Cmd::List {
            library,
            shoot,
            verdict,
            photos,
            min_stars,
            best,
        } => {
            let lib = Library::open(&library)?;
            if best {
                let args = serde_json::json!({ "shoot_id": shoot });
                print_parts(
                    Tools::new(&library).call("best_photos", args)?,
                    Path::new(""),
                )?;
            } else if photos || shoot.is_some() || verdict.is_some() || min_stars.is_some() {
                let mut list = lib.list_photos(&PhotoFilter { shoot, verdict })?;
                list.retain(|p| p.stars >= min_stars.unwrap_or(0));
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&lib.shoots()?)?);
            }
        }
        Cmd::Backup { library, list } => {
            let lib = Library::open(&library)?;
            if list {
                println!("{}", serde_json::to_string_pretty(&lib.backups()?)?);
            } else {
                println!("{}", serde_json::json!({ "backup": lib.backup_now()? }));
            }
        }
        Cmd::Restore { name, library } => {
            let before = Library::open(&library)?.restore_backup(&name)?;
            println!(
                "{}",
                serde_json::json!({ "restored": name, "previous_state": before })
            );
        }
        Cmd::Serve { library, port } => serve::run(&library, port)?,
        Cmd::Mcp { library } => mcp::run(&library)?,
        Cmd::Edit {
            library,
            args,
            relative,
        } => {
            let (ids, settings): (Vec<&String>, Vec<&String>) =
                args.iter().partition(|a| a.parse::<i64>().is_ok());
            let ids: Vec<i64> = ids.iter().map(|a| a.parse().unwrap()).collect();
            let mut set = serde_json::Map::new();
            for kv in settings {
                let (k, v) = parse_setting(kv)?;
                set.insert(k.to_string(), serde_json::json!(v));
            }
            let args = serde_json::json!({ "photo_ids": ids, "set": set, "relative": relative });
            print_parts(
                Tools::new(&library).call("edit_recipe", args)?,
                Path::new(""),
            )?;
        }
        Cmd::Export {
            ids,
            library,
            to,
            format,
            long_edge,
            quality,
            naming,
        } => {
            let args = serde_json::json!({ "photo_ids": ids, "folder": std::path::absolute(&to)?,
                "format": format, "long_edge": long_edge, "quality": quality, "naming": naming });
            print_parts(
                Tools::new(&library).call("export_photos", args)?,
                Path::new(""),
            )?;
        }
        Cmd::Tool {
            name,
            args,
            library,
            out,
        } => {
            let args: serde_json::Value = serde_json::from_str(&args)?;
            print_parts(Tools::new(&library).call(&name, args)?, &out)?;
        }
        Cmd::Cull {
            library,
            shoot,
            provider,
            compute,
        } => {
            let mut lib = Library::open(&library)?;
            let shoot = match shoot {
                Some(s) => s,
                None => lib
                    .shoots()?
                    .last()
                    .map(|s| s.id)
                    .ok_or("the Library has no Shoots")?,
            };
            let provider = argus_engine::cull::provider(provider.as_deref(), compute)?;
            let t = Instant::now();
            let report = argus_engine::cull::cull_shoot_with(
                &mut lib,
                shoot,
                provider.as_ref(),
                argus_engine::cull::max_per_request(),
                &|p| eprintln!("{}", p.line()),
            )?;
            let mut v = serde_json::to_value(&report)?;
            v["total_ms"] = serde_json::json!(ms(t));
            println!("{v}");
        }
        Cmd::Agent {
            prompt,
            library,
            photo,
            shoot,
            provider,
            compute,
        } => {
            let ask = argus_engine::agent::Ask {
                prompt,
                shoot_id: shoot,
                photo_id: photo,
                provider,
                compute,
                ..Default::default()
            };
            let t = Instant::now();
            let reply = argus_engine::agent::run(&Tools::new(&library), &ask)?;
            let mut v = serde_json::to_value(&reply)?;
            v["total_ms"] = serde_json::json!(ms(t));
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Work {
            library,
            once,
            drain,
            compute,
        } => {
            use argus_engine::jobs::{self, Gate};
            let tools = Tools::new(&library);
            let lib = Library::open(&library)?;
            lib.requeue_stale_jobs()?;
            let pause = || std::thread::sleep(std::time::Duration::from_secs(2));
            loop {
                let cfg = jobs::config(&lib).unwrap_or_default();
                let (c, min, held) = if drain {
                    (jobs::effective_compute(&cfg), i64::MIN, &[][..])
                } else {
                    match jobs::gate(&lib, &cfg) {
                        Gate::Run {
                            compute,
                            min_priority,
                            held,
                        } => (compute, min_priority, held),
                        Gate::Wait(why) if once => {
                            eprintln!("argus work: {why}; nothing started (--drain runs anyway)");
                            break;
                        }
                        Gate::Wait(_) => {
                            pause();
                            continue;
                        }
                    }
                };
                match jobs::run_next_holding(&tools, compute.unwrap_or(c), min, held)? {
                    Some(job) => println!("{}", serde_json::to_string(&job)?),
                    None if once => {
                        let left = lib.queue_summary()?.queued;
                        if left > 0 {
                            eprintln!(
                                "argus work: {left} queued jobs wait for overnight, the charger \
                                 or Run now, as in the app (--drain runs them now)"
                            );
                        }
                        break;
                    }
                    None => pause(),
                }
            }
        }
        Cmd::Flag {
            verdict,
            ids,
            library,
            reason,
            ai,
        } => {
            let mut lib = Library::open(&library)?;
            let author = if ai { Author::Ai } else { Author::Person };
            lib.set_verdict(&ids, verdict, author, reason.as_deref(), None)?;
            println!(
                "{{\"flagged\":{},\"verdict\":\"{}\"}}",
                ids.len(),
                verdict.as_str()
            );
        }
        Cmd::Rate {
            stars,
            ids,
            library,
            ai,
        } => {
            let mut lib = Library::open(&library)?;
            let author = if ai { Author::Ai } else { Author::Person };
            lib.set_rating(&ids, stars, author, None)?;
            println!("{{\"rated\":{},\"stars\":{stars}}}", ids.len());
        }
    }
    Ok(())
}

/// `key=value` with a numeric value.
fn parse_setting(kv: &str) -> Result<(&str, f64), Box<dyn std::error::Error>> {
    let (k, v) = kv
        .split_once('=')
        .ok_or_else(|| format!("expected setting=value, got {kv:?}"))?;
    let v: f64 = v
        .trim_start_matches('+')
        .parse()
        .map_err(|_| format!("{k}: {v:?} is not a number"))?;
    Ok((k.trim(), v))
}

/// Print a tool's JSON parts; write JPEG parts to `out` (`out-2.jpg`, ... when several).
fn print_parts(
    parts: Vec<argus_engine::tools::Part>,
    out: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use argus_engine::tools::Part;
    let mut n = 0;
    for p in parts {
        match p {
            Part::Json(v) => println!("{}", serde_json::to_string_pretty(&v)?),
            Part::Jpeg(bytes) => {
                n += 1;
                let path = if n == 1 {
                    out.to_path_buf()
                } else {
                    let stem = out.file_stem().unwrap_or_default().to_string_lossy();
                    out.with_file_name(format!("{stem}-{n}.jpg"))
                };
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&path, bytes)?;
                eprintln!("wrote {}", path.display());
            }
        }
    }
    Ok(())
}

const JPEG_QUALITY: u8 = 90;

/// Encode by extension: .jpg/.jpeg -> JPEG (quality 90), anything else -> PNG.
fn write_image(out: &Path, img: &RgbImage) -> Result<(), Box<dyn std::error::Error>> {
    let ext = out
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "jpg" || ext == "jpeg" {
        let mut enc = jpeg_encoder::Encoder::new_file(out, JPEG_QUALITY)?;
        enc.set_sampling_factor(jpeg_encoder::SamplingFactor::F_2_2); // 4:2:0, as cameras do
        enc.encode(
            &img.data,
            img.width as u16,
            img.height as u16,
            jpeg_encoder::ColorType::Rgb,
        )?;
    } else {
        let mut enc = png::Encoder::new(
            BufWriter::new(std::fs::File::create(out)?),
            img.width as u32,
            img.height as u32,
        );
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header()?;
        w.write_image_data(&img.data)?;
        w.finish()?;
    }
    Ok(())
}

fn serde_line(file: &std::path::Path, w: usize, h: usize, t: &[(&str, f64)]) -> String {
    let mut s = format!(
        "{{\"file\":{:?},\"width\":{w},\"height\":{h}",
        file.display().to_string()
    );
    for (k, v) in t {
        s += &format!(",\"{k}\":{v}");
    }
    s + "}"
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Long-running commands keep a Debug log and Resource meter beside the app's
    // (~/Library/Logs/Argus on a Mac; elsewhere only when ARGUS_LOG_DIR says where).
    let long = matches!(
        cli.cmd,
        Cmd::Serve { .. }
            | Cmd::Mcp { .. }
            | Cmd::Cull { .. }
            | Cmd::Agent { .. }
            | Cmd::Work { .. }
            | Cmd::Tool { .. }
            | Cmd::Import { .. }
            | Cmd::Previews { .. }
    );
    if long && (cfg!(target_os = "macos") || std::env::var_os("ARGUS_LOG_DIR").is_some()) {
        if let Some(dir) = argus_engine::logs::default_dir() {
            let cmd = std::env::args().nth(1).unwrap_or_default();
            argus_engine::logs::start_debug(&dir, &format!("argus {cmd}"));
        }
    }
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `argus denoise`: see [`Cmd::Denoise`].
fn denoise_time(
    file: &Path,
    tiles: usize,
    backend: &str,
    compare: bool,
    compute: Compute,
    out: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use argus_engine::denoise::{self, Backend};
    use argus_engine::pipeline;
    let backend = match backend {
        "auto" => Backend::best(),
        "cpu" => Backend::Cpu,
        "coreml" => Backend::CoreMl,
        other => return Err(format!("--backend {other}: auto, cpu or coreml").into()),
    };
    let raw = decode(file)?;
    let (w, h) = raw.crop_size();
    let full = pipeline::demosaic_bilinear(&pipeline::normalize_white_balance(&raw), w, h, raw.cfa);
    // A centre crop about `tiles` tiles big (224 px apart), or the whole frame.
    let side = if tiles == 0 {
        w.max(h)
    } else {
        ((tiles as f64).sqrt() * 224.0) as usize
    };
    let (cw, ch) = (side.min(w), side.min(h));
    let (x0, y0) = ((w - cw) / 2, (h - ch) / 2);
    let mut crop = Vec::with_capacity(cw * ch * 3);
    for y in y0..y0 + ch {
        crop.extend_from_slice(&full[(y * w + x0) * 3..(y * w + x0 + cw) * 3]);
    }
    drop(full);
    let pool = compute.cpu_pool()?;
    let run = |backend: Backend| {
        let started = std::time::Instant::now();
        pool.install(|| {
            denoise::denoise_on(backend, &crop, cw, ch, &|done, total| {
                println!("::progress {done}/{total} tiles ({})", backend.name())
            })
        })
        .map(|den| (den, started.elapsed().as_secs_f64()))
    };
    let (den, secs) = run(backend)?;
    // The CPU result is the reference: how far this backend's base strays from it, in
    // linear 0..1 values (an 8-bit sRGB step in the mid-tones is about 0.002).
    let vs_cpu = if compare && backend != Backend::Cpu {
        let (cpu, cpu_secs) = run(Backend::Cpu)?;
        let diffs: Vec<f32> = den.iter().zip(&cpu).map(|(a, b)| (a - b).abs()).collect();
        let mut sorted = diffs.clone();
        sorted.sort_by(f32::total_cmp);
        serde_json::json!({
            "cpu_seconds": (cpu_secs * 10.0).round() / 10.0,
            "speedup": (cpu_secs / secs * 10.0).round() / 10.0,
            "mean_diff": diffs.iter().map(|&d| d as f64).sum::<f64>() / diffs.len().max(1) as f64,
            "p999_diff": sorted.get(sorted.len() * 999 / 1000).copied().unwrap_or(0.0),
            "max_diff": sorted.last().copied().unwrap_or(0.0),
        })
    } else {
        serde_json::Value::Null
    };
    let per_tile = secs / denoise::tiles(cw, ch) as f64;
    std::fs::create_dir_all(out)?;
    let stem = file
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut half = crop.clone();
    denoise::blend(&mut half, &den, denoise::weight(50.0));
    for (name, rgb) in [("before", crop), ("amount50", half), ("after", den)] {
        let d = argus_engine::Demosaiced {
            width: cw,
            height: ch,
            rgb,
            camera_to_srgb: pipeline::camera_to_srgb_matrix(raw.xyz_to_cam),
            orientation: raw.orientation,
            scale: 1.0,
            lens: Default::default(),
        };
        let img = d.finish(&pipeline::Look::DEFAULT, &Recipe::default(), None);
        std::fs::write(
            out.join(format!("{stem}_{name}.jpg")),
            argus_engine::render::encode_jpeg(&img, 92)?,
        )?;
    }
    println!(
        "{}",
        serde_json::json!({
            "file": file,
            "size": [w, h],
            "crop": [cw, ch],
            "tiles": denoise::tiles(cw, ch),
            "seconds": (secs * 10.0).round() / 10.0,
            "seconds_per_tile": (per_tile * 1000.0).round() / 1000.0,
            "backend": backend.name(),
            "units": std::env::var("ARGUS_COREML_UNITS").unwrap_or_else(|_| "all".into()),
            "threads": pool.current_num_threads(),
            "compute": format!("{compute:?}").to_lowercase(),
            "vs_cpu": vs_cpu,
            "whole_frame_tiles": denoise::tiles(w, h),
            "whole_frame_minutes": (per_tile * denoise::tiles(w, h) as f64 / 6.0).round() / 10.0,
        })
    );
    Ok(())
}
