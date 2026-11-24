import { useTranslation } from 'react-i18next';
import styles from './import.module.css';
import type { StepId } from './guide-steps';
import type { StepMark } from './capture/steps/step-state';

/** One step of the bar: how it stands (null: not done, or it has no state), its summary, and if it can be opened. */
export interface StepInfo {
  id: StepId;
  mark: StepMark | null;
  summary?: string;
  reachable: boolean;
}

interface StepBarProps {
  steps: StepInfo[];
  current: StepId;
  onJump: (id: StepId) => void;
  /** True while something is running that must not be interrupted: no step can be opened. */
  disabled?: boolean;
}

/** The steps of the guide as a list of buttons: the current one marked, the done ones ticked and summarised. */
export function StepBar({ steps, current, onJump, disabled = false }: StepBarProps) {
  const { t } = useTranslation();
  return (
    <nav aria-label={t('import.guide.steps')}>
      <ol className={styles.stepBar}>
        {steps.map((step) => (
          <li key={step.id}>
            <button
              type="button"
              className={styles.stepButton}
              aria-current={step.id === current ? 'step' : undefined}
              disabled={disabled || !step.reachable}
              onClick={() => onJump(step.id)}
            >
              <span>
                {t(`import.guide.stepName.${step.id}`)}
                {step.mark === 'done' && (
                  <>
                    {' '}
                    <span aria-hidden="true">{t('import.guide.tick')}</span>
                    <span className={styles.visuallyHidden}>{t('import.guide.done')}</span>
                  </>
                )}
              </span>
              {step.mark === 'skipped' && (
                <span className={styles.stepMark}>{t('import.guide.skipped')}</span>
              )}
              {step.mark === 'attention' && (
                <span className={styles.stepMark}>{t('import.guide.attention')}</span>
              )}
              {step.mark === 'done' && step.summary && (
                <span className={styles.stepSummary}>{step.summary}</span>
              )}
            </button>
          </li>
        ))}
      </ol>
    </nav>
  );
}
