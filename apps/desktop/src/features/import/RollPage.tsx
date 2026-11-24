import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ImportGuideEntry, ImportSource, Roll } from '../../shared/ipc';
import { RollForm } from '../library/RollForm';
import { isStepId } from './guide-steps';
import styles from './import.module.css';
import { useFocusHeading } from './use-focus-heading';

const SOURCES: readonly ImportSource[] = ['camera', 'scanner', 'files'];

interface RollPageProps {
  rolls: Roll[];
  /** The guided import entry of each roll, by roll id. */
  entries: Record<string, ImportGuideEntry>;
  rollId: string | null;
  source: ImportSource | null;
  /** Whether camera capture can be chosen (the guided capture flag is on). */
  cameraOn: boolean;
  onRoll: (id: string) => void;
  onSource: (source: ImportSource) => void;
}

/** Rolls that were worked on in the guide first, each group newest first. */
function ordered(rolls: Roll[], entries: Record<string, ImportGuideEntry>): Roll[] {
  const newest = [...rolls].sort((a, b) => b.created_at.localeCompare(a.created_at));
  return [...newest.filter((r) => r.id in entries), ...newest.filter((r) => !(r.id in entries))];
}

/** The first page of the guide: the roll (an existing one, or a new one made here) and where its frames come from. */
export function RollPage({
  rolls,
  entries,
  rollId,
  source,
  cameraOn,
  onRoll,
  onSource,
}: RollPageProps) {
  const { t } = useTranslation();
  const headingRef = useRef<HTMLHeadingElement>(null);
  useFocusHeading(headingRef, true);
  const [creating, setCreating] = useState(false);
  // With no roll in the library the form is all there is to do.
  const showForm = creating || rolls.length === 0;

  return (
    <section aria-labelledby="import-roll" className={styles.section}>
      <h3 id="import-roll" ref={headingRef} tabIndex={-1} className={styles.heading}>
        {t('import.guide.roll.heading')}
      </h3>
      <p className={styles.muted}>{t('import.guide.roll.intro')}</p>

      {rolls.length > 0 && (
        <fieldset className={styles.choices}>
          <legend>{t('import.guide.roll.legend')}</legend>
          {ordered(rolls, entries).map((roll) => {
            const step = entries[roll.id]?.step;
            return (
              <label key={roll.id} className={styles.choice}>
                <input
                  type="radio"
                  name="import-roll"
                  checked={!showForm && roll.id === rollId}
                  onChange={() => {
                    setCreating(false);
                    onRoll(roll.id);
                  }}
                />
                <span>
                  {roll.name}
                  {step !== undefined && isStepId(step) && (
                    <span className={styles.resume}>
                      {' '}
                      {t('import.guide.roll.resumeAt', {
                        step: t(`import.guide.stepName.${step}`),
                      })}
                    </span>
                  )}
                </span>
              </label>
            );
          })}
          <label className={styles.choice}>
            <input
              type="radio"
              name="import-roll"
              checked={showForm}
              onChange={() => setCreating(true)}
            />
            <span>{t('import.guide.roll.new')}</span>
          </label>
        </fieldset>
      )}

      {showForm && (
        <RollForm
          key="new"
          roll={null}
          submitLabel={t('import.guide.roll.create')}
          onSaved={(saved) => {
            setCreating(false);
            onRoll(saved.id);
          }}
          onCancel={rolls.length > 0 ? () => setCreating(false) : undefined}
        />
      )}

      <fieldset className={styles.choices}>
        <legend>{t('import.guide.source.legend')}</legend>
        {SOURCES.map((s) => (
          <label key={s} className={styles.choice}>
            <input
              type="radio"
              name="import-source"
              checked={source === s}
              disabled={s === 'camera' && !cameraOn}
              onChange={() => onSource(s)}
            />
            <span>{t(`import.guide.source.${s}`)}</span>
          </label>
        ))}
        {!cameraOn && <p className={styles.muted}>{t('import.guide.source.cameraOff')}</p>}
      </fieldset>
    </section>
  );
}
