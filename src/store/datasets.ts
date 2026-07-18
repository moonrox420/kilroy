/**
 * Datasets store — UI state for the dataset import / custom-model flow.
 *
 * Holds the "is the dialog open?" flag and the in-flight inspection /
 * creation state. The actual data shapes (DatasetInspect, ModelfileBuilt,
 * TrainingEnv) live in `@/lib/tauri` and we don't duplicate them here —
 * components consume them directly from the typed binding.
 */
import { create } from "zustand";
import type {
  CreateProgress,
  DatasetInspect,
  ModelfileBuilt,
  TrainingEnv,
} from "@/lib/tauri";

interface DatasetsState {
  open: boolean;
  inspecting: boolean;
  /** Last inspection result. Cleared when a new file is picked. */
  inspect: DatasetInspect | null;
  /** Error from the most recent inspect / create attempt. */
  error: string | null;
  creating: boolean;
  createProgress: CreateProgress | null;
  /** Result from `dataset_create_modelfile` once it resolves. Sticky so
   *  the UI can show "Created — switch to it?" after the dialog closes
   *  if we ever decide to surface it elsewhere. */
  lastBuild: ModelfileBuilt | null;
  trainingEnv: TrainingEnv | null;

  /** A path the next dialog mount should auto-inspect. Set by callers
   *  who want to deep-link into the flow (e.g. CorpusBanner pointing
   *  at `.kilroy/corpus/training.jsonl`). Consumed once and cleared. */
  pendingPath: string | null;

  openDialog: () => void;
  /** Open the dialog with a path that should be auto-inspected on
   *  mount. Pass null/undefined to open without auto-load. */
  openDialogWithPath: (path: string) => void;
  consumePendingPath: () => string | null;
  closeDialog: () => void;
  setInspecting: (b: boolean) => void;
  setInspect: (i: DatasetInspect | null) => void;
  setError: (e: string | null) => void;
  setCreating: (b: boolean) => void;
  setCreateProgress: (p: CreateProgress | null) => void;
  setLastBuild: (b: ModelfileBuilt | null) => void;
  setTrainingEnv: (e: TrainingEnv | null) => void;
  reset: () => void;
}

export const useDatasets = create<DatasetsState>((set, get) => ({
  open: false,
  inspecting: false,
  inspect: null,
  error: null,
  creating: false,
  createProgress: null,
  lastBuild: null,
  trainingEnv: null,
  pendingPath: null,

  openDialog: () => set({ open: true }),
  openDialogWithPath: (path) => set({ open: true, pendingPath: path }),
  consumePendingPath: () => {
    const p = get().pendingPath;
    if (p) set({ pendingPath: null });
    return p;
  },
  closeDialog: () => set({ open: false }),
  setInspecting: (b) => set({ inspecting: b }),
  setInspect: (i) => set({ inspect: i, error: null }),
  setError: (e) => set({ error: e }),
  setCreating: (b) => set({ creating: b }),
  setCreateProgress: (p) => set({ createProgress: p }),
  setLastBuild: (b) => set({ lastBuild: b }),
  setTrainingEnv: (e) => set({ trainingEnv: e }),
  reset: () =>
    set({
      inspect: null,
      error: null,
      creating: false,
      createProgress: null,
      lastBuild: null,
    }),
}));
