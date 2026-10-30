import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { useUiStore } from './app/store';

vi.mock('./shared/ipc', () => ({
  libraryInfo: () => Promise.resolve({ app_version: '1.2.3' }),
}));

describe('App shell', () => {
  beforeEach(() => {
    useUiStore.setState({ view: 'library' });
  });

  it('starts on the Library view and shows the backend version', async () => {
    render(<App />);
    expect(screen.getByRole('heading', { level: 2, name: 'Library' })).toBeInTheDocument();
    expect(await screen.findByText('Version 1.2.3')).toBeInTheDocument();
  });

  it('switches between Library, Develop and Import', async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: 'Develop' }));
    expect(screen.getByRole('heading', { level: 2, name: 'Develop' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Develop' })).toHaveAttribute('aria-current', 'page');

    await user.click(screen.getByRole('button', { name: 'Import' }));
    expect(screen.getByRole('heading', { level: 2, name: 'Import' })).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Library' }));
    expect(screen.getByRole('heading', { level: 2, name: 'Library' })).toBeInTheDocument();
  });
});
