//! Whole-Shoot jobs that fan out into per-Photo work: a Shoot's Suggestions and
//! "apply to the rest".

use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ShootArgs {
    shoot_id: Option<ShootId>,
    photo_ids: Option<Vec<PhotoId>>,
    count: Option<usize>,
    provider: Option<String>,
    #[serde(default)]
    redo: bool,
    /// Model takes for every Photo that isn't a Reject, not only the likely keepers.
    #[serde(default)]
    all: bool,
    /// Prepare ([`crate::prepare`]): also queue a denoised base for each target that is
    /// noisy enough to want one.
    #[serde(default)]
    prepare: bool,
}

/// Stars from which a frame counts as a likely keeper. The cull's Taste gives kept frames
/// 3 to 5 stars for the top 70% of the Shoot and 2 for the rest.
pub const KEEPER_STARS: u8 = 3;

/// Whether a Shoot's Suggestions pass gives `p` model takes now. The rest keep Auto and the
/// Edit head from Import, which Greg's blind taps rate level with the model's Natural take
/// (29 to 28, 2026-10-22), and get model takes when he opens them ([`focus`]). A model
/// call is about 200 s a Photo on his Mac, so this roughly halves a card's AI pass.
/// An unrated frame (no cull yet) has nothing to go on, so it counts as one; a burst's
/// later frames wait for the person unless picked. `leads` says whether `p` is the first
/// frame of its burst that isn't a Reject (so a rejected cover doesn't leave the burst
/// without takes).
pub(super) fn likely_keeper(p: &crate::library::Photo, leads: bool) -> bool {
    p.verdict == Verdict::Pick
        || p.best.is_some()
        || ((p.stars == 0 || p.stars >= KEEPER_STARS) && leads)
}

/// The Photos of a Shoot worth editing, best first: the AI's Best Photos in rank order,
/// then Picks, then by stars, then capture order. Rejects are left out.
pub(super) fn worth_editing(lib: &Library, shoot: ShootId) -> Result<Vec<crate::library::Photo>> {
    let mut photos: Vec<_> = lib
        .list_photos(&crate::library::PhotoFilter {
            shoot: Some(shoot),
            verdict: None,
        })?
        .into_iter()
        .filter(|p| p.verdict != Verdict::Reject)
        .collect();
    // A stable sort keeps capture order among equals.
    photos.sort_by_key(|p| {
        (
            p.best.unwrap_or(i64::MAX),
            p.verdict != Verdict::Pick,
            std::cmp::Reverse(p.stars),
            p.stack_rank.unwrap_or(0) > 0,
        )
    });
    Ok(photos)
}

/// Whether `photo` already has fresh (not matched) Suggestions.
pub(super) fn has_fresh(lib: &Library, photo: PhotoId) -> Result<bool> {
    Ok(lib
        .suggestions(Some(photo), None, false)?
        .iter()
        .any(|s| s.reference_id.is_none()))
}

pub(super) fn newest_shoot(lib: &Library) -> Result<ShootId> {
    lib.shoots()?
        .iter()
        .map(|s| s.id)
        .max()
        .ok_or_else(|| Error("the Library has no Shoots yet".into()))
}

pub(super) fn suggest_shoot(tools: &Tools, job: &Job) -> Result<Value> {
    let a: ShootArgs = args(job)?;
    let mut lib = Library::open(tools.library())?;
    let mut on_open = 0;
    let targets: Vec<(PhotoId, bool)> = match a.photo_ids {
        Some(ids) => ids.into_iter().map(|id| (id, false)).collect(),
        None => {
            let shoot = match a.shoot_id {
                Some(s) => s,
                None => newest_shoot(&lib)?,
            };
            let all = worth_editing(&lib, shoot)?;
            let pairs = |v: Vec<crate::library::Photo>| -> Vec<(PhotoId, bool)> {
                v.into_iter()
                    .map(|p| (p.id, p.verdict == Verdict::Pick))
                    .collect()
            };
            if a.all {
                lib.set_setting(&on_open_key(shoot), &json!([]))?;
                pairs(all)
            } else {
                // Each burst's lowest-ranked frame left after Rejects leads it.
                let mut lead: std::collections::HashMap<i64, i64> = Default::default();
                for p in &all {
                    if let Some(s) = p.stack {
                        let r = p.stack_rank.unwrap_or(0);
                        lead.entry(s).and_modify(|m| *m = (*m).min(r)).or_insert(r);
                    }
                }
                let (now, later): (Vec<_>, Vec<_>) = all.into_iter().partition(|p| {
                    let leads = p
                        .stack
                        .is_none_or(|s| lead.get(&s) == Some(&p.stack_rank.unwrap_or(0)));
                    likely_keeper(p, leads)
                });
                // A frame that already has fresh takes needs none when opened.
                let mut wait = Vec::new();
                for p in later {
                    if a.redo || !has_fresh(&lib, p.id)? {
                        wait.push(p.id);
                    }
                }
                lib.set_setting(&on_open_key(shoot), &json!(wait))?;
                on_open = wait.len();
                pairs(now)
            }
        }
    };
    let mut queued = Vec::new();
    let mut skipped = 0;
    let total = targets.len() as i64;
    let keepers: Vec<PhotoId> = targets.iter().map(|t| t.0).collect();
    for (i, (photo, pick)) in targets.into_iter().enumerate() {
        if !a.redo && has_fresh(&lib, photo)? {
            skipped += 1;
            continue;
        }
        let mut args = json!({ "photo_id": photo });
        if let Some(c) = a.count {
            args["count"] = json!(c);
        }
        if let Some(p) = &a.provider {
            args["provider"] = json!(p);
        }
        let priority = if pick { PRIORITY_PICK } else { PRIORITY_OTHER };
        let (id, _) = enqueue(&mut lib, "suggest", &args, Some(priority), None)?;
        queued.push(id);
        lib.job_progress(job.id, i as i64 + 1, total)?;
    }
    if a.prepare {
        crate::denoise::queue_keepers(&mut lib, &keepers)?;
    }
    Ok(json!({ "queued": queued, "skipped": skipped, "on_open": on_open }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HeroArgs {
    photo_id: PhotoId,
    photo_ids: Option<Vec<PhotoId>>,
    provider: Option<String>,
}

/// "Apply to the rest": the hero's whole style (its Look and its own sliders) becomes a
/// new Look of the Shoot; every target gets matched Suggestions under it. Nothing changes
/// until the person accepts them.
pub(super) fn match_hero(tools: &Tools, job: &Job) -> Result<Value> {
    let a: HeroArgs = args(job)?;
    let mut lib = Library::open(tools.library())?;
    let hero = lib.find_photo(a.photo_id)?;
    let style = lib.with_look(&hero, &lib.current_recipe(hero.id)?)?.global;
    let stem = hero
        .file_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(&hero.file_name)
        .to_string();
    let looks = lib.looks(hero.shoot_id)?;
    let base_id: String = format!("from-{stem}")
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut look_id = base_id.clone();
    let mut n = 2;
    while looks.iter().any(|l| l.id == look_id) {
        look_id = format!("{base_id}-{n}");
        n += 1;
    }
    let look = crate::library::Look {
        id: look_id.clone(),
        name: format!("Look from {stem}"),
        adjust: style,
    };
    lib.save_look(hero.shoot_id, &look, None)?;

    let targets: Vec<PhotoId> = match a.photo_ids {
        Some(ids) => ids,
        None => worth_editing(&lib, hero.shoot_id)?
            .into_iter()
            .map(|p| p.id)
            .collect(),
    };
    let mut queued = Vec::new();
    for id in targets.into_iter().filter(|&id| id != hero.id) {
        let mut args =
            json!({ "photo_id": id, "reference_id": hero.id, "look_id": look_id, "count": 2 });
        if let Some(p) = &a.provider {
            args["provider"] = json!(p);
        }
        // A newer hero replaces an older one's queued matching for the same Photo.
        let key = format!("suggest:{id}:match");
        if let Some(old) = lib
            .jobs(
                Some(crate::library::JobStatus::Queued),
                Some("suggest"),
                10_000,
            )?
            .into_iter()
            .find(|j| j.args.get("reference_id").is_some() && j.args["photo_id"] == json!(id))
        {
            lib.cancel_job(old.id)?;
        }
        let (jid, _) = enqueue(&mut lib, "suggest", &args, Some(PRIORITY_MATCH), Some(&key))?;
        queued.push(jid);
    }
    Ok(json!({ "look_id": look_id, "queued": queued }))
}
