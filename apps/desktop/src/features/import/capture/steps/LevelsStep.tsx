import { useTranslation } from 'react-i18next';
import styles from '../../import.module.css';
import { Calibrate } from '../Calibrate';
import { sameLevels, useCaptureRig } from '../CaptureRig';
import { LevelSliders } from '../LevelSliders';
import { SaveLevels } from '../SaveLevels';
import { TestShot } from '../TestShot';
import { RigLiveView } from './RigLiveView';
import { levelsState } from './step-state';
import { StepSection } from './StepSection';

/** Step "Light levels": the levels of the light, calibrated, tried with a test shot and saved. */
export function LevelsStep() {
  const { t } = useTranslation();
  const rig = useCaptureRig();
  const { levels, saved, roll, stock } = rig;
  const state = levelsState(rig);

  const showDefaultNotice =
    levels !== null &&
    saved?.source === 'default' &&
    !rig.levelsHandled &&
    sameLevels(levels, saved.levels);
  // The notice button is disabled whenever the Calibrate button is.
  const calibrateDisabled =
    !(rig.usable && !rig.settingsChanging) ||
    rig.running ||
    rig.calibrating ||
    roll === null ||
    levels === null;
  // The global levels can be chosen while the levels are the default ones and none is chosen yet.
  const canUseGlobal =
    levels !== null &&
    saved?.source === 'default' &&
    sameLevels(levels, saved.levels) &&
    !rig.globalLevels;

  return (
    <StepSection id="import-levels" heading={t('import.steps.levels.heading')}>
      <p className={styles.muted}>{t('import.steps.levels.intro')}</p>
      <RigLiveView />
      {levels && (
        <>
          <LevelSliders
            levels={levels}
            onChange={(next) => {
              rig.setLevels(next);
              rig.setShotExposure(null);
              rig.setLevelsHandled(true);
            }}
            disabled={rig.running}
          />
          <p role="status">
            {saved && sameLevels(levels, saved.levels)
              ? saved.source === 'stock'
                ? t('import.capture.levels.source.stock', { stock: saved.stock ?? '' })
                : t('import.capture.levels.source.default')
              : t('import.capture.levels.source.changed')}
          </p>
          {showDefaultNotice && (
            <div role="status" className={styles.captureNotice}>
              <p>
                {t(
                  stock
                    ? 'import.capture.levels.defaultNotice'
                    : 'import.capture.levels.defaultNoticeNoStock',
                )}
              </p>
              <button
                type="button"
                disabled={calibrateDisabled}
                onClick={() => rig.calibrateStart.current?.()}
              >
                {t('import.capture.levels.defaultNoticeAction')}
              </button>
            </div>
          )}
          {canUseGlobal && (
            <div className={styles.buttons}>
              <button
                type="button"
                disabled={rig.busy}
                onClick={() => {
                  rig.setGlobalLevels(true);
                  rig.setLevelsHandled(true);
                }}
              >
                {t('import.steps.levels.useGlobal')}
              </button>
            </div>
          )}
          {state.mark === 'attention' && (
            <p role="alert">{t('import.steps.levels.settingsChanged')}</p>
          )}
          <TestShot
            disabled={rig.running || !rig.usable || rig.settingsChanging}
            levels={levels}
            onShot={(exposure) => rig.setShotExposure(exposure)}
            onBusyChange={rig.setShotBusy}
          />
          <Calibrate
            area={rig.calibrationArea}
            onClearArea={() => rig.setCalibrationArea(null)}
            ready={rig.usable && !rig.settingsChanging}
            busy={rig.running}
            roll={roll}
            levels={levels}
            onBusyChange={rig.setCalibrating}
            startRef={rig.calibrateStart}
            onCalibrated={(found, exposure) => {
              rig.setLevels(found);
              rig.setShotExposure(exposure);
              rig.setCalibrated({ levels: found, exposure });
              rig.setLevelsHandled(true);
            }}
          />
          <SaveLevels
            rollId={rig.rollId}
            levels={levels}
            saved={saved}
            stock={stock}
            shot={rig.shotExposure ? { exposure: rig.shotExposure } : null}
            disabled={rig.running}
            onSaved={rig.setSaved}
          />
        </>
      )}
      {state.mark === 'done' && rig.globalLevels && saved?.source === 'default' && (
        <p role="status">{t('import.steps.levels.usingGlobal')}</p>
      )}
    </StepSection>
  );
}
