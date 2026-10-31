import * as Dialog from '@radix-ui/react-dialog';
import { ProgressList, Slider } from '@gtp/ui';
import { useEffect, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { exportStart, jobCancel, pickFolder, type ExportOptions } from '../../shared/ipc';
import { describeJob } from './JobsPanel';
import styles from './library.module.css';
import { messageOf, useLibraryStore } from './library-store';

type Format = ExportOptions['format'];
type Space = ExportOptions['space'];

const FORMATS: readonly Format[] = ['tiff16', 'jpeg'];
const SPACES: readonly Space[] = ['srgb', 'adobe_rgb', 'prophoto'];
const DEFAULT_TEMPLATE = '{roll}_{index:03}';
const STORAGE_KEY = 'gtp.export-options';

const DEFAULTS: ExportOptions = {
  format: 'tiff16',
  space: 'srgb',
  jpeg_quality: 92,
  dest_dir: '',
  template: DEFAULT_TEMPLATE,
};

/** The options used last time in this window, or the defaults; anything odd in storage is ignored. */
function loadOptions(): ExportOptions {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null');
    if (typeof raw !== 'object' || raw === null) return DEFAULTS;
    const o = raw as Record<string, unknown>;
    return {
      format: FORMATS.find((f) => f === o.format) ?? DEFAULTS.format,
      space: SPACES.find((s) => s === o.space) ?? DEFAULTS.space,
      jpeg_quality:
        typeof o.jpeg_quality === 'number' &&
        Number.isInteger(o.jpeg_quality) &&
        o.jpeg_quality >= 1 &&
        o.jpeg_quality <= 100
          ? o.jpeg_quality
          : DEFAULTS.jpeg_quality,
      dest_dir: typeof o.dest_dir === 'string' ? o.dest_dir : DEFAULTS.dest_dir,
      template:
        typeof o.template === 'string' && o.template !== '' ? o.template : DEFAULTS.template,
    };
  } catch {
    return DEFAULTS;
  }
}

function saveOptions(options: ExportOptions) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(options));
  } catch {
    // Not remembered: the dialog works the same.
  }
}

/** What the file name would be for a roll "Roll", frame 1; null when the template is not valid. */
export function exampleName(template: string, format: Format): string | null {
  const extension = format === 'tiff16' ? 'tif' : 'jpg';
  const filled = template
    .replace(/\{roll\}/g, 'Roll')
    .replace(/\{index:0([1-9])\}/g, (_, width: string) => '1'.padStart(Number(width), '0'))
    .replace(/\{index\}/g, '1')
    .replace(/\{frame\}/g, '01J0000000000000000000000')
    .replace(/\{ext\}/g, extension);
  if (/[{}]/.test(filled) || filled.trim() === '') return null;
  return filled.toLowerCase().endsWith(`.${extension}`) ? filled : `${filled}.${extension}`;
}

/** The translation key of what is wrong with the options, or null. */
function problemOf(options: ExportOptions): string | null {
  if (options.dest_dir.trim() === '') return 'export.errors.noFolder';
  if (options.template.trim() === '') return 'export.errors.noTemplate';
  if (/[\\/]/.test(options.template)) return 'export.errors.folders';
  return null;
}

interface ExportDialogProps {
  /** The frames to export. */
  frameIds: readonly string[];
  open: boolean;
  onClose: () => void;
}

/**
 * The export dialog: format, colour space, quality, destination folder and file name, then the
 * progress of the job, a summary of what was written and what failed, and cancel. Closing it does
 * not stop the job; reopening it shows the export that is running.
 */
export function ExportDialog({ frameIds, open, onClose }: ExportDialogProps) {
  const { t, i18n } = useTranslation();
  const addJob = useLibraryStore((s) => s.addJob);
  const dismissJob = useLibraryStore((s) => s.dismissJob);
  const running = useLibraryStore((s) =>
    s.jobs.find((j) => j.kind === 'export' && j.state === 'running'),
  );

  const [options, setOptions] = useState<ExportOptions>(loadOptions);
  const [jobId, setJobId] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const job = useLibraryStore((s) => s.jobs.find((j) => j.id === jobId));

  // Opening the dialog while an export runs shows that export.
  useEffect(() => {
    if (open && jobId === null && running) setJobId(running.id);
  }, [open, jobId, running]);

  const change = (patch: Partial<ExportOptions>) => setOptions((o) => ({ ...o, ...patch }));
  const problem = problemOf(options);
  const folderProblem = problem === 'export.errors.noFolder';
  const example = exampleName(options.template, options.format);

  const choose = async () => {
    setError(null);
    try {
      const picked = await pickFolder();
      if (picked) change({ dest_dir: picked });
    } catch (e) {
      setError(messageOf(e));
    }
  };

  const start = async (event: FormEvent) => {
    event.preventDefault();
    if (problem !== null || starting) return;
    setStarting(true);
    setError(null);
    try {
      const chosen = { ...options, template: options.template.trim() };
      saveOptions(chosen);
      const id = await exportStart([...frameIds], chosen);
      addJob({
        id,
        kind: 'export',
        rollName: '',
        frames: frameIds.length,
        destination: chosen.dest_dir,
        fraction: 0,
        state: 'running',
      });
      setJobId(id);
    } catch (e) {
      setError(t('export.startFailed', { message: messageOf(e) }));
    } finally {
      setStarting(false);
    }
  };

  const finished = job !== undefined && job.state !== 'running';

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content className={styles.dialog}>
          <Dialog.Title>{t('export.title')}</Dialog.Title>
          <Dialog.Description>
            {t('export.description', { count: job?.frames ?? frameIds.length })}
          </Dialog.Description>

          {job ? (
            <div className={styles.stack}>
              <ProgressList
                items={[describeJob(job, t, i18n.language)]}
                label={t('jobs.label')}
                cancelLabel={t('jobs.cancel')}
                dismissLabel={t('jobs.dismiss')}
                onCancel={(id) => void jobCancel(id)}
                onDismiss={(id) => {
                  dismissJob(id);
                  setJobId(null);
                }}
              />
              {job.state === 'done' && job.exported && (
                <p role="status">
                  {t('export.summary', {
                    count: job.exported.length,
                    folder: job.destination ?? '',
                  })}
                </p>
              )}
              {job.state === 'running' && <p className={styles.hint}>{t('export.runningHint')}</p>}
              <div className={styles.buttons}>
                {finished && (
                  <button type="button" onClick={() => setJobId(null)}>
                    {t('export.again')}
                  </button>
                )}
                <button type="button" onClick={onClose}>
                  {t('export.close')}
                </button>
              </div>
            </div>
          ) : (
            <form onSubmit={start} className={styles.form}>
              <fieldset className={styles.fieldset}>
                <legend>{t('export.format')}</legend>
                {FORMATS.map((format) => (
                  <label key={format} className={styles.radio}>
                    <input
                      type="radio"
                      name="format"
                      value={format}
                      checked={options.format === format}
                      onChange={() => change({ format })}
                    />
                    {t(`export.${format}`)}
                    <span className={styles.hint}>{t(`export.${format}Hint`)}</span>
                  </label>
                ))}
              </fieldset>

              <label className={styles.field}>
                {t('export.space')}
                <select
                  value={options.space}
                  onChange={(e) => change({ space: e.target.value as Space })}
                >
                  {SPACES.map((space) => (
                    <option key={space} value={space}>
                      {t(`export.${space}`)}
                    </option>
                  ))}
                </select>
                <span className={styles.hint}>{t(`export.spaceHint.${options.space}`)}</span>
              </label>

              {options.format === 'jpeg' && (
                <Slider
                  label={t('export.quality')}
                  value={options.jpeg_quality}
                  min={1}
                  max={100}
                  step={1}
                  onChange={(jpeg_quality) => change({ jpeg_quality })}
                />
              )}

              <div className={styles.folderRow}>
                <span>{t('export.folder')}</span>
                <span className={styles.path}>{options.dest_dir || t('export.noFolder')}</span>
                <button type="button" onClick={() => void choose()}>
                  {t('export.choose')}
                </button>
              </div>

              <label className={styles.field}>
                {t('export.template')}
                <input
                  type="text"
                  value={options.template}
                  maxLength={200}
                  spellCheck={false}
                  aria-invalid={problem !== null && !folderProblem}
                  onChange={(e) => change({ template: e.target.value })}
                />
                <span className={styles.hint}>{t('export.templateHint')}</span>
                {example && (
                  <span className={styles.hint}>{t('export.example', { name: example })}</span>
                )}
              </label>

              {problem !== null && !folderProblem && <p role="alert">{t(problem)}</p>}
              {error && <p role="alert">{error}</p>}
              <div className={styles.buttons}>
                <button type="submit" disabled={problem !== null || starting}>
                  {starting ? t('export.starting') : t('export.start')}
                </button>
                <button type="button" onClick={onClose}>
                  {t('export.close')}
                </button>
              </div>
            </form>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
