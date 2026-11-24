import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiStore } from '../../app/store';
import {
  settingsGet,
  settingsSet,
  type ImportGuideEntry,
  type ImportSource,
} from '../../shared/ipc';
import { useLibraryStore } from '../library/library-store';
import { flagsOf } from '../settings/flags';
import { CaptureSection } from './CaptureSection';
import { FilesSection } from './FilesSection';
import { isStepId, steps, type StepId } from './guide-steps';
import { useGuideStore } from './guide-store';
import { RollPage } from './RollPage';
import { ScannerSection } from './ScannerSection';
import { StepBar, type StepInfo } from './StepBar';
import styles from './import.module.css';

type Entries = Record<string, ImportGuideEntry>;

/** Whether the key press goes to something being typed in, where Alt and an arrow are the text's. */
function typingIn(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) return true;
  if (target instanceof HTMLInputElement) {
    return !['radio', 'checkbox', 'button', 'submit'].includes(target.type);
  }
  return false;
}

/**
 * Importing as a guide: the Roll page, then the pages of the roll's source (one each for now). The
 * roll's source and last step are kept in the settings, so coming back resumes there; whether a step
 * is done is worked out from what is chosen, never stored.
 */
export function ImportGuide() {
  const { t } = useTranslation();
  const rolls = useLibraryStore((s) => s.rolls);
  const selectRoll = useLibraryStore((s) => s.selectRoll);
  const setView = useUiStore((s) => s.setView);
  const setLastRoll = useGuideStore((s) => s.setLastRoll);

  // The saved entry of every roll; null until the settings are read.
  const [entries, setEntries] = useState<Entries | null>(null);
  const [rollId, setRollId] = useState<string | null>(null);
  const [source, setSource] = useState<ImportSource | null>(null);
  const [step, setStep] = useState<StepId>('roll');
  // Camera capture is offered only while the guided capture flag is on.
  const [cameraOn, setCameraOn] = useState(false);

  // Reads the settings once and resumes the roll worked on last in this session.
  useEffect(() => {
    let alive = true;
    settingsGet()
      .then(
        (settings) => {
          if (alive) setCameraOn(flagsOf(settings).guided_capture);
          return settings.import_guide ?? {};
        },
        () => ({}),
      )
      .then((saved: Entries) => {
        if (!alive) return;
        setEntries(saved);
        const last = useGuideStore.getState().lastRollId;
        const roll = useLibraryStore.getState().rolls.find((r) => r.id === last);
        const entry = roll ? saved[roll.id] : undefined;
        if (!roll) return;
        setRollId(roll.id);
        if (!entry) return;
        setSource(entry.source);
        if (isStepId(entry.step) && steps(entry.source, roll.film_type).includes(entry.step)) {
          setStep(entry.step);
        }
      });
    return () => {
      alive = false;
    };
  }, []);

  // A roll that was deleted elsewhere is forgotten.
  useEffect(() => {
    if (rollId !== null && !rolls.some((r) => r.id === rollId)) {
      setRollId(null);
      setSource(null);
      setStep('roll');
    }
  }, [rolls, rollId]);

  const roll = rolls.find((r) => r.id === rollId) ?? null;
  // A saved camera source does not count while camera capture is off.
  const usable = source === 'camera' && !cameraOn ? null : source;
  const ids = usable && roll ? steps(usable, roll.film_type) : (['roll'] as StepId[]);
  const index = Math.max(0, ids.indexOf(step));

  /** Done when it can be told: the Roll step by its roll and source; the last steps have no state yet. */
  const doneOf = (id: StepId): boolean | null =>
    id === 'roll' ? roll !== null && usable !== null : null;
  const firstOpen = ids.findIndex((id) => doneOf(id) !== true);

  const rollSummary = (): string | undefined => {
    if (!roll) return undefined;
    const stock = roll.film_stock?.trim();
    return [roll.name, ...(stock ? [stock] : []), t(`rollEditor.filmTypes.${roll.film_type}`)].join(
      t('import.guide.summary.separator'),
    );
  };

  const infos: StepInfo[] = ids.map((id, i) => ({
    id,
    done: doneOf(id),
    summary: id === 'roll' ? rollSummary() : undefined,
    reachable: firstOpen === -1 || i <= firstOpen,
  }));

  /** Records where a roll is in the guide, in memory and in the settings. */
  const save = (id: string, entry: ImportGuideEntry) => {
    setEntries((all) => ({ ...all, [id]: entry }));
    settingsSet({ import_guide_entry: { roll_id: id, entry } }).catch(() => {
      // The place is only a convenience: it stays in memory when the settings cannot be saved.
    });
  };

  const goTo = (next: StepId) => {
    setStep(next);
    if (rollId !== null && source !== null) save(rollId, { source, step: next });
  };

  const chooseRoll = (id: string) => {
    setRollId(id);
    setLastRoll(id);
    setStep('roll');
    const entry = entries?.[id];
    if (entry) {
      setSource(entry.source);
    } else if (source !== null) {
      save(id, { source, step: 'roll' });
    }
  };

  const chooseSource = (next: ImportSource) => {
    setSource(next);
    setStep('roll');
    if (rollId !== null) save(rollId, { source: next, step: 'roll' });
  };

  const canBack = index > 0;
  const isLast = index === ids.length - 1;
  const canNext = doneOf(ids[index] ?? 'roll') !== false;

  const next = () => {
    if (!roll || !source) return;
    if (isLast) {
      selectRoll(roll.id);
      setView('library');
      return;
    }
    // From the Roll page, a roll that was worked on before goes back to the step it was at.
    const saved = entries?.[roll.id];
    const kept = saved && saved.source === source && isStepId(saved.step) ? saved.step : null;
    goTo(
      index === 0 && kept && ids.includes(kept) && kept !== 'roll'
        ? kept
        : (ids[index + 1] ?? step),
    );
  };
  const back = () => {
    if (canBack) goTo(ids[index - 1] ?? 'roll');
  };

  // Alt and the left or right arrow go back and on, unless text is being typed or the button is off.
  const actions = useRef({ next, back, canNext, canBack });
  actions.current = { next, back, canNext, canBack };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      if (event.key !== 'ArrowRight' && event.key !== 'ArrowLeft') return;
      if (typingIn(event.target)) return;
      // Alt and an arrow are ours here: a radio button must not move on, nor the page go back.
      event.preventDefault();
      const now = actions.current;
      if (event.key === 'ArrowRight' ? !now.canNext : !now.canBack) return;
      if (event.key === 'ArrowRight') now.next();
      else now.back();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  if (entries === null) return null;

  const page = (() => {
    switch (ids[index]) {
      case 'capture':
        return roll && <CaptureSection rollId={roll.id} />;
      case 'scan':
        return roll && <ScannerSection rollId={roll.id} />;
      case 'files':
        return roll && <FilesSection rollId={roll.id} />;
      default:
        return (
          <RollPage
            rolls={rolls}
            entries={entries}
            rollId={rollId}
            source={source}
            cameraOn={cameraOn}
            onRoll={chooseRoll}
            onSource={chooseSource}
          />
        );
    }
  })();

  return (
    <div className={styles.guide}>
      <StepBar
        steps={infos}
        current={ids[index] ?? 'roll'}
        onJump={(id) => {
          if (infos.find((i) => i.id === id)?.reachable) goTo(id);
        }}
      />
      {page}
      <div className={styles.guideButtons}>
        <button type="button" disabled={!canBack} onClick={back}>
          {t('import.guide.back')}
        </button>
        <button type="button" disabled={!canNext || !roll || !source} onClick={next}>
          {isLast && roll && source ? t('import.guide.finish') : t('import.guide.next')}
        </button>
      </div>
    </div>
  );
}
