import { create } from 'zustand';

export type View = 'library' | 'develop' | 'import' | 'settings';

/** The frame open in the Develop view: its id and its position in the Library's current view. */
export interface DevelopFrame {
  id: string;
  index: number;
}

interface UiState {
  view: View;
  /** The frame open in Develop, or null when none was chosen yet. */
  developFrame: DevelopFrame | null;
  setView: (view: View) => void;
  /** Opens a frame in the Develop view (from the Library grid or the filmstrip). */
  openInDevelop: (id: string, index: number) => void;
}

export const useUiStore = create<UiState>((set) => ({
  view: 'library',
  developFrame: null,
  setView: (view) => set({ view }),
  openInDevelop: (id, index) => set({ view: 'develop', developFrame: { id, index } }),
}));
