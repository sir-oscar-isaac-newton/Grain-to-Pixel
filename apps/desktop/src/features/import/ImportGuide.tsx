import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiStore } from '../../app/store';
import {
  settingsGet,
  settingsSet,
  type ImportGuideEntry,
  type ImportSource,
  type Roll,
} from '../../shared/ipc';
import { useLibraryStore } from '../library/library-store';
import { CaptureRigProvider, useOptionalCaptureRig } from './capture/CaptureRig';
import { CaptureStep } from './capture/steps/CaptureStep';
import { FlatFieldStep } from './capture/steps/FlatFieldStep';
import { FocusStep } from './capture/steps/FocusStep';
import { LevelsStep } from './capture/steps/LevelsStep';
import { SetupStep } from './capture/steps/SetupStep';
import { cameraStepState, type StepState } from './capture/steps/step-state';
import { flagsOf } from '../settings/flags';
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
 * Importing as a guide: the Roll page, then the pages of the roll's source (the camera has five). The
 * roll's source and last step are kept in the settings, so coming back resumes there; whether a step
 * is done is worked out from what is chosen, never stored.
 */
export function ImportGuide() {
  const rolls = useLibraryStore((s) => s.rolls);
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

  /** Records where a roll is in the guide, in memory and in the settings. */
  const save = (id: string, entry: ImportGuideEntry) => {
    setEntries((all) => ({ ...all, [id]: entry }));
    settingsSet({ import_guide_entry: { roll_id: id, entry } }).catch(() => {
      // The place is only a convenience: it stays in memory when the settings cannot be saved.
    });
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

  if (entries === null) return null;

  // The camera steps share one rig (light, camera, levels) for as long as the guide is on them.
  return (
    <CaptureRigProvider rollId={usable === 'camera' && roll ? roll.id : null}>
      <GuideBody
        rolls={rolls}
        entries={entries}
        roll={roll}
        source={source}
        usable={usable}
        cameraOn={cameraOn}
        step={step}
        ids={ids}
        onStep={(next) => {
          setStep(next);
          if (rollId !== null && source !== null) save(rollId, { source, step: next });
        }}
        onRoll={chooseRoll}
        onSource={chooseSource}
      />
    </CaptureRigProvider>
  );
}

interface GuideBodyProps {
  rolls: Roll[];
  entries: Entries;
  roll: Roll | null;
  source: ImportSource | null;
  /** The source, but null while it cannot be used. */
  usable: ImportSource | null;
  cameraOn: boolean;
  step: StepId;
  ids: StepId[];
  onStep: (step: StepId) => void;
  onRoll: (id: string) => void;
  onSource: (source: ImportSource) => void;
}

const NO_STATE: StepState = { mark: null, canPass: true };

/** The step bar, the page of the step and the Back and Next buttons, inside the camera rig when there is one. */
function GuideBody({
  rolls,
  entries,
  roll,
  source,
  usable,
  cameraOn,
  step,
  ids,
  onStep,
  onRoll,
  onSource,
}: GuideBodyProps) {
  const { t } = useTranslation();
  const selectRoll = useLibraryStore((s) => s.selectRoll);
  const setView = useUiStore((s) => s.setView);
  const rig = useOptionalCaptureRig();
  const index = Math.max(0, ids.indexOf(step));
  // While the camera is shooting, nothing may move on or back.
  const busy = rig?.busy ?? false;

  /** How a step stands: the Roll step by its roll and source, the camera steps by the rig. */
  const stateOf = (id: StepId): StepState => {
    if (id === 'roll') {
      return roll !== null && usable !== null
        ? { mark: 'done', canPass: true }
        : { mark: null, canPass: false };
    }
    return rig ? cameraStepState(id, rig) : NO_STATE;
  };
  const firstBlocking = ids.findIndex((id) => !stateOf(id).canPass);

  const rollSummary = (): string | undefined => {
    if (!roll) return undefined;
    const stock = roll.film_stock?.trim();
    return [roll.name, ...(stock ? [stock] : []), t(`rollEditor.filmTypes.${roll.film_type}`)].join(
      t('import.guide.summary.separator'),
    );
  };

  const infos: StepInfo[] = ids.map((id, i) => ({
    id,
    mark: stateOf(id).mark,
    summary: id === 'roll' ? rollSummary() : undefined,
    reachable: firstBlocking === -1 || i <= firstBlocking,
  }));

  /** Moves to a step; the camera is left as it should be (live view stopped, the light off). */
  const goTo = (to: StepId) => {
    if (busy) return;
    if (to !== step) rig?.leaveStep();
    onStep(to);
  };

  const canBack = index > 0 && !busy;
  const isLast = index === ids.length - 1;
  const canNext = stateOf(ids[index] ?? 'roll').canPass && !busy;

  const next = () => {
    if (!roll || !source) return;
    if (isLast) {
      rig?.leaveStep();
      selectRoll(roll.id);
      setView('library');
      return;
    }
    // From the Roll page, a roll that was worked on before goes back to the step it was at, but no
    // further than the first step that must be done first (after a restart the light is not
    // connected, so that is Set up again).
    const saved = entries[roll.id];
    const kept = saved && saved.source === source && isStepId(saved.step) ? saved.step : null;
    const keptAt = kept && kept !== 'roll' ? ids.indexOf(kept) : -1;
    const resumeAt = keptAt > 0 && firstBlocking > 0 ? Math.min(keptAt, firstBlocking) : keptAt;
    goTo(index === 0 && resumeAt > 0 ? (ids[resumeAt] ?? step) : (ids[index + 1] ?? step));
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

  const page = (() => {
    switch (ids[index]) {
      case 'setup':
        return rig && <SetupStep />;
      case 'focus':
        return rig && <FocusStep />;
      case 'flatfield':
        return rig && <FlatFieldStep onSkipped={() => actions.current.next()} />;
      case 'levels':
        return rig && <LevelsStep />;
      case 'capture':
        return rig && <CaptureStep />;
      case 'scan':
        return roll && <ScannerSection rollId={roll.id} />;
      case 'files':
        return roll && <FilesSection rollId={roll.id} />;
      default:
        return (
          <RollPage
            rolls={rolls}
            entries={entries}
            rollId={roll?.id ?? null}
            source={source}
            cameraOn={cameraOn}
            onRoll={onRoll}
            onSource={onSource}
          />
        );
    }
  })();

  return (
    <div className={styles.guide}>
      <StepBar
        steps={infos}
        current={ids[index] ?? 'roll'}
        disabled={busy}
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
