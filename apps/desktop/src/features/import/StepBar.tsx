import { useTranslation } from 'react-i18next';
import styles from './import.module.css';
import type { StepId } from './guide-steps';

/** One step of the bar: whether it is done (null: it has no done state), its summary, and if it can be opened. */
export interface StepInfo {
  id: StepId;
  done: boolean | null;
  summary?: string;
  reachable: boolean;
}

interface StepBarProps {
  steps: StepInfo[];
  current: StepId;
  onJump: (id: StepId) => void;
}

/** The steps of the guide as a list of buttons: the current one marked, the done ones ticked and summarised. */
export function StepBar({ steps, current, onJump }: StepBarProps) {
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
              disabled={!step.reachable}
              onClick={() => onJump(step.id)}
            >
              <span>
                {t(`import.guide.stepName.${step.id}`)}
                {step.done === true && (
                  <>
                    {' '}
                    <span aria-hidden="true">{t('import.guide.tick')}</span>
                    <span className={styles.visuallyHidden}>{t('import.guide.done')}</span>
                  </>
                )}
              </span>
              {step.done === true && step.summary && (
                <span className={styles.stepSummary}>{step.summary}</span>
              )}
            </button>
          </li>
        ))}
      </ol>
    </nav>
  );
}
