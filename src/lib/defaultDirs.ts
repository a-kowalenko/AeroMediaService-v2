/** Helpers for wizard default AeroMediaService root (Archiv + Logs). */

import {
  ensureDefaultAppRoot,
  proposeDefaultDirs,
  type EnsureDefaultAppRootResult,
} from "@/lib/tauri";

export type ApplyDefaultAppRootResult = {
  ensured: EnsureDefaultAppRootResult;
};

/**
 * Propose → optional warnings confirm → create AeroMediaService/{Archiv,Logs}.
 * Returns null when the user cancels a confirmation prompt.
 */
export async function applyDefaultAppRoot(opts?: {
  confirmWarnings?: (warnings: string[], path: string) => boolean;
}): Promise<ApplyDefaultAppRootResult | null> {
  const proposal = await proposeDefaultDirs();

  if (proposal.warnings.length > 0) {
    const ok = opts?.confirmWarnings
      ? opts.confirmWarnings(proposal.warnings, proposal.root)
      : window.confirm(
          [
            "Hinweise zum Standardordner:",
            ...proposal.warnings.map((w) => `• ${w}`),
            "",
            `Ordner anlegen: ${proposal.root}`,
            "(enthält Archiv und Logs)",
            "",
            "Trotzdem fortfahren?",
          ].join("\n"),
        );
    if (!ok) return null;
  }

  const ensured = await ensureDefaultAppRoot(proposal.root);
  return { ensured };
}

/** Infer AeroMediaService root from configured Archiv/Log paths. */
export function inferAppRoot(
  archivePath: string,
  logPath: string,
  standardRoot?: string | null,
): string {
  const archive = archivePath.trim().replace(/[/\\]+$/, "");
  const log = logPath.trim().replace(/[/\\]+$/, "");
  const stripLeaf = (p: string, leaf: string) => {
    const re = new RegExp(`[/\\\\]${leaf}$`, "i");
    return re.test(p) ? p.replace(re, "") : "";
  };
  const fromArchive = stripLeaf(archive, "Archiv");
  const fromLog = stripLeaf(log, "Logs");
  if (fromArchive && fromLog && fromArchive.toLowerCase() === fromLog.toLowerCase()) {
    return fromArchive;
  }
  if (fromArchive) return fromArchive;
  if (fromLog) return fromLog;
  return standardRoot?.trim() || "";
}
