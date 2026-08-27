import type {IdAssignOverride, IdAssignPreview} from "@/lib/tauri";

export function overrideFromPreview(preview: IdAssignPreview): IdAssignOverride {
  return {
    tandemmaster: preview.tandemmaster?.trim() || null,
    videospringer: preview.videospringer?.trim() || null,
    dropzone_suffix: preview.dropzone_suffix?.trim() || null,
  };
}
