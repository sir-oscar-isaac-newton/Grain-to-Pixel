import { create } from 'zustand';
import {
  IpcError,
  libraryCreate,
  libraryInfo,
  libraryOpen,
  rollList,
  settingsGet,
  tagList,
  type LibraryDetails,
  type Roll,
  type SkippedFile,
  type Tag,
} from '../../shared/ipc';
import { NO_FILTERS, type Filters } from './search-query';
import { resetThumbnails } from './thumbnails';

export type Phase = 'loading' | 'needs-library' | 'ready';
export type JobState = 'running' | 'done' | 'error' | 'cancelled';

/** A background job started from this screen (an import), as the progress list shows it. */
export interface JobRow {
  id: string;
  kind: 'import';
  rollName: string;
  fraction: number;
  state: JobState;
  imported?: number;
  skipped?: SkippedFile[];
  /** The backend's message when the job failed. */
  message?: string;
}

/** A line of text for the user, as a translation key with values (so the store holds no text). */
export interface Notice {
  key: string;
  values?: Record<string, string | number>;
}

interface LibraryState {
  phase: Phase;
  library: LibraryDetails | null;
  /** Why the last library could not be reopened at start, if it could not. */
  startError: string | null;
  rolls: Roll[];
  /** Every tag of the library, in name order. */
  tags: Tag[];
  filters: Filters;
  /** The roll shown, or null for every frame. */
  rollId: string | null;
  /** Bumped when frames were added, changed or removed, so the grid reloads. */
  revision: number;
  selected: ReadonlySet<string>;
  jobs: JobRow[];
  /** Updates for jobs the screen has not registered yet (a fast job can report before it is added). */
  unclaimed: Record<string, Partial<JobRow>>;
  notice: Notice | null;

  start: () => Promise<void>;
  createLibrary: (path: string, name: string) => Promise<void>;
  openLibrary: (path: string) => Promise<void>;
  reloadRolls: () => Promise<void>;
  reloadTags: () => Promise<void>;
  setFilters: (filters: Filters) => void;
  selectRoll: (id: string | null) => void;
  setSelected: (ids: Iterable<string>) => void;
  bump: () => void;
  addJob: (job: JobRow) => void;
  patchJob: (id: string, patch: Partial<JobRow>) => void;
  dismissJob: (id: string) => void;
  setNotice: (notice: Notice | null) => void;
}

const initial = {
  phase: 'loading' as Phase,
  library: null,
  startError: null,
  rolls: [],
  tags: [],
  filters: NO_FILTERS,
  rollId: null,
  revision: 0,
  selected: new Set<string>() as ReadonlySet<string>,
  jobs: [],
  unclaimed: {},
  notice: null,
};

export function messageOf(error: unknown): string {
  return error instanceof IpcError || error instanceof Error ? error.message : String(error);
}

export const useLibraryStore = create<LibraryState>((set, get) => {
  /** Makes `library` the open one and loads its rolls. */
  async function enter(library: LibraryDetails) {
    resetThumbnails();
    set({
      phase: 'ready',
      library,
      startError: null,
      rollId: null,
      filters: NO_FILTERS,
      tags: [],
      selected: new Set(),
      jobs: [],
      unclaimed: {},
      notice: null,
      revision: get().revision + 1,
    });
    await Promise.all([get().reloadRolls(), get().reloadTags()]);
  }

  return {
    ...initial,

    /** Opens what is already open, or the last library, or asks for one. */
    async start() {
      set({ phase: 'loading', startError: null });
      try {
        const info = await libraryInfo();
        if (info.library) return await enter(info.library);
      } catch {
        // Outside the Tauri shell (tests, plain browser) there is no backend: ask for a library.
      }
      let startError: string | null = null;
      try {
        const last = (await settingsGet()).last_library;
        if (last) {
          const info = await libraryOpen(last);
          if (info.library) return await enter(info.library);
        }
      } catch (error) {
        startError = messageOf(error);
      }
      set({ phase: 'needs-library', startError });
    },

    /** Creates and opens a library; rejects with the backend's error so the dialog can show it. */
    async createLibrary(path, name) {
      const info = await libraryCreate(path, name);
      if (info.library) await enter(info.library);
    },

    async openLibrary(path) {
      const info = await libraryOpen(path);
      if (info.library) await enter(info.library);
    },

    async reloadRolls() {
      const rolls = await rollList();
      const { rollId } = get();
      set({
        rolls,
        rollId: rollId && rolls.some((r) => r.id === rollId) ? rollId : null,
      });
    },

    /** Reloads the tags; a tag that no longer exists is dropped from the filters. */
    async reloadTags() {
      const tags = await tagList();
      const known = new Set(tags.map((t) => t.id));
      const { filters } = get();
      const tagIds = filters.tagIds.filter((id) => known.has(id));
      set({
        tags,
        filters: tagIds.length === filters.tagIds.length ? filters : { ...filters, tagIds },
      });
    },

    setFilters(filters) {
      set({ filters, selected: new Set() });
    },

    selectRoll(id) {
      set({ rollId: id, selected: new Set() });
    },

    setSelected(ids) {
      set({ selected: new Set(ids) });
    },

    bump() {
      set({ revision: get().revision + 1 });
    },

    addJob(job) {
      const early = get().unclaimed[job.id];
      const unclaimed = Object.fromEntries(
        Object.entries(get().unclaimed).filter(([id]) => id !== job.id),
      );
      set({
        jobs: [{ ...job, ...early }, ...get().jobs.filter((j) => j.id !== job.id)],
        unclaimed,
      });
    },

    patchJob(id, patch) {
      if (!get().jobs.some((j) => j.id === id)) {
        set({ unclaimed: { ...get().unclaimed, [id]: { ...get().unclaimed[id], ...patch } } });
        return;
      }
      set({ jobs: get().jobs.map((j) => (j.id === id ? { ...j, ...patch } : j)) });
    },

    dismissJob(id) {
      set({ jobs: get().jobs.filter((j) => j.id !== id) });
    },

    setNotice(notice) {
      set({ notice });
    },
  };
});

/** Back to the starting state (tests). */
export function resetLibraryStore() {
  useLibraryStore.setState({ ...initial, selected: new Set() });
}
