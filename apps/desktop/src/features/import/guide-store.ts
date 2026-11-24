import { create } from 'zustand';

interface GuideState {
  /** The roll the guide worked on last in this session, so leaving Import and coming back resumes it. */
  lastRollId: string | null;
  setLastRoll: (id: string | null) => void;
}

export const useGuideStore = create<GuideState>((set) => ({
  lastRollId: null,
  setLastRoll: (lastRollId) => set({ lastRollId }),
}));

/** Forgets the last roll (tests). */
export function resetGuideStore() {
  useGuideStore.setState({ lastRollId: null });
}
