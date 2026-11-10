import { useEffect, useState, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { frameGet, frameSetNote } from '../../shared/ipc';
import styles from './library.module.css';
import { messageOf, useLibraryStore } from './library-store';

/** The note of the one selected frame: read when it is chosen, saved on blur and on Enter. */
export function FrameNote({ frameId }: { frameId: string }) {
  const { t } = useTranslation();
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  const bump = useLibraryStore((s) => s.bump);
  const setNotice = useLibraryStore((s) => s.setNotice);
  /** The note as stored, or null until it is read. */
  const [saved, setSaved] = useState<string | null>(null);
  const [draft, setDraft] = useState('');

  useEffect(() => {
    let current = true;
    frameGet(frameId).then(
      (frame) => {
        if (!current) return;
        setSaved(frame.note ?? '');
        setDraft(frame.note ?? '');
      },
      () => {
        // The note is a convenience; without it the field stays empty and locked.
      },
    );
    return () => {
      current = false;
    };
  }, [frameId]);

  const save = async () => {
    if (readOnly || saved === null || draft === saved) return;
    try {
      const frame = await frameSetNote(frameId, draft);
      setSaved(frame.note ?? '');
      bump();
    } catch (e) {
      setNotice({ key: 'library.notice.noteFailed', values: { message: messageOf(e) } });
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // Enter saves; Shift+Enter adds a line.
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      void save();
    }
  };

  return (
    <label className={styles.frameNote}>
      <span>{t('frameNote.label')}</span>
      <textarea
        rows={2}
        value={draft}
        readOnly={readOnly}
        disabled={saved === null}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => void save()}
        onKeyDown={onKeyDown}
      />
    </label>
  );
}
