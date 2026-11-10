import { useTranslation } from 'react-i18next';

interface ResetPanelButtonProps {
  /** The panel's name, for the accessible name ("Reset Colour to automatic"). */
  panel: string;
  disabled: boolean;
  onClick: () => void;
}

/** The button in a panel's header that puts the panel's settings back to the automatic ones. */
export function ResetPanelButton({ panel, disabled, onClick }: ResetPanelButtonProps) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      disabled={disabled}
      aria-label={t('develop.resetPanel.label', { panel })}
      onClick={onClick}
    >
      {t('develop.resetPanel.text')}
    </button>
  );
}
