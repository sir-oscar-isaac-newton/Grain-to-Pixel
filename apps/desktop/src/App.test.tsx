import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { useUiStore } from './app/store';

const crashNotesTake = vi.fn();
vi.mock('./features/library/LibraryView', async () => {
  const { createElement } = await import('react');
  return { LibraryView: () => createElement('h2', null, 'Library') };
});

vi.mock('./features/settings/SettingsView', async () => {
  const { createElement } = await import('react');
  return { SettingsView: () => createElement('h2', null, 'Settings') };
});

vi.mock('./shared/ipc', () => ({
  libraryInfo: () => Promise.resolve({ app_version: '1.2.3', library: null }),
  crashNotesTake: () => crashNotesTake(),
}));

describe('App shell', () => {
  beforeEach(() => {
    useUiStore.setState({ view: 'library', developFrame: null });
    crashNotesTake.mockReset();
    crashNotesTake.mockResolvedValue([]);
  });

  it('tells the user when the previous run crashed, and the notice can be dismissed', async () => {
    crashNotesTake.mockResolvedValue([{ file: 'crash-1-2.txt' }]);
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByRole('status')).toHaveTextContent('closed unexpectedly');
    await user.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('shows no crash notice when the previous run was clean', async () => {
    render(<App />);
    await screen.findByText('Version 1.2.3');
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('starts on the Library view and shows the backend version', async () => {
    render(<App />);
    expect(screen.getByRole('heading', { level: 2, name: 'Library' })).toBeInTheDocument();
    expect(await screen.findByText('Version 1.2.3')).toBeInTheDocument();
  });

  it('switches between Library, Develop, Import and Settings', async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: 'Develop' }));
    expect(screen.getByRole('heading', { level: 2, name: 'Develop' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Develop' })).toHaveAttribute('aria-current', 'page');
    // No frame was chosen: the view says how to open one.
    expect(screen.getByText(/double-click it, or select it and press Enter/)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Import' }));
    expect(screen.getByRole('heading', { level: 2, name: 'Import' })).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByRole('heading', { level: 2, name: 'Settings' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Settings' })).toHaveAttribute(
      'aria-current',
      'page',
    );

    await user.click(screen.getByRole('button', { name: 'Library' }));
    expect(screen.getByRole('heading', { level: 2, name: 'Library' })).toBeInTheDocument();
  });
});
