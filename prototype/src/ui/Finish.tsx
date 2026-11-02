// Finish (GLOSSARY): the end of a Shoot in one go, from Export mode. Keywords and the backup
// waited for this, so Prepare left the Mac free for the cull; Finish queues them, and Run it
// all runs them with the exports at Full (Greg, 2026-11-02: one sitting, heavy work at the
// ends). Mocked in the one-sitting page as "Export, then Finish".

import { useState } from "react";
import { toast } from "sonner";
import { CheckCheck } from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Button } from "@/components/ui/button";
import { finishShoot } from "../data/prepare";
import { openSettings } from "../settings/Settings";
import { go } from "../router";
import { Kbd } from "./kit";

export function FinishShoot({
  shootId,
  shootName,
}: {
  shootId: number | null;
  shootName: string;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const finish = async () => {
    if (shootId == null) return;
    setBusy(true);
    try {
      const r = await finishShoot(shootId);
      setOpen(false);
      const backups = r.backups.length;
      toast(
        backups
          ? `Finishing ${shootName}: Keywords and ${backups === 1 ? "a backup" : `${backups} backups`}, at Full`
          : `Finishing ${shootName}: Keywords at Full. No backup place yet.`,
        {
          id: "argus",
          action: backups
            ? { label: "Work ⌘5", onClick: () => go({ name: "work" }) }
            : { label: "Add one", onClick: () => openSettings("storage") },
        },
      );
    } catch (e) {
      toast(`Couldn’t finish: ${String(e).replace(/^Error: /, "")}`, {
        id: "argus",
      });
    } finally {
      setBusy(false);
    }
  };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          size="sm"
          variant="ghost"
          disabled={shootId == null}
          data-testid="finish-open"
        >
          <CheckCheck size={12} /> Finish
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="w-[320px] p-3 text-[12px]"
        data-testid="finish-popover"
        onKeyDown={(e) =>
          e.key === "Enter" && !busy && (e.preventDefault(), void finish())
        }
      >
        <div className="wm-h">Finish {shootName}</div>
        <p className="faint" style={{ margin: "6px 0 10px" }}>
          The work Prepare left for the end runs now, at Full, keeping the Mac
          awake. Walk away when it starts.
        </p>
        <ul className="finish-list">
          <li>Keywords for every Photo, for search later</li>
          <li>
            A copy of the Library in each backup place (Settings › Storage and
            backup)
          </li>
          <li>Anything else still waiting, like late denoise</li>
        </ul>
        <div className="finish-foot">
          <span className="faint">Export first, then Finish</span>
          <span className="grow" />
          <Button
            size="sm"
            onClick={() => void finish()}
            disabled={busy}
            data-testid="finish-run"
          >
            {busy ? "Starting…" : "Finish"} <Kbd>↵</Kbd>
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
