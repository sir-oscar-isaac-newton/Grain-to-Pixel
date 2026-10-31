import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { TagChips } from '@gtp/ui';
import { framesOwnTags, tagApply, type Tag } from '../../shared/ipc';
import styles from './library.module.css';
import { messageOf, useLibraryStore } from './library-store';

/**
 * Tags for the selected frames: the tags they carry themselves (a tag on only some of them shows
 * too, and removing it takes it off all) and an input to add one to every selected frame.
 */
export function SelectionTags() {
  const { t } = useTranslation();
  const selected = useLibraryStore((s) => s.selected);
  const tags = useLibraryStore((s) => s.tags);
  const revision = useLibraryStore((s) => s.revision);
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  const bump = useLibraryStore((s) => s.bump);
  const reloadTags = useLibraryStore((s) => s.reloadTags);
  const setNotice = useLibraryStore((s) => s.setNotice);

  const [own, setOwn] = useState<Tag[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let current = true;
    framesOwnTags([...selected]).then(
      (found) => current && setOwn(found),
      () => current && setOwn([]),
    );
    return () => {
      current = false;
    };
  }, [selected, revision]);

  const apply = async (add: string[], remove: string[]) => {
    setBusy(true);
    try {
      await tagApply([...selected], add, remove);
      bump();
      await reloadTags();
    } catch (e) {
      setNotice({ key: 'library.notice.tagFailed', values: { message: messageOf(e) } });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={styles.selectionTags}>
      <span>{t('library.selectionTags.title', { count: selected.size })}</span>
      <TagChips
        label={t('library.selectionTags.label')}
        tags={own}
        emptyText={t('library.selectionTags.none')}
        removeLabel={(name) => t('library.selectionTags.remove', { name })}
        onRemove={(tag) => void apply([], [tag.name])}
        onAdd={(name) => void apply([name], [])}
        addLabel={t('library.selectionTags.add')}
        addButtonLabel={t('library.selectionTags.addButton')}
        suggestions={tags.map((tag) => tag.name)}
        disabled={readOnly || busy}
      />
    </div>
  );
}
