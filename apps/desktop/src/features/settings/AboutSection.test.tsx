import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../test-support';
import { AboutSection } from './AboutSection';

describe('AboutSection', () => {
  it('says that rawler is used under the LGPL-2.1 and shows the licence and the credits', () => {
    render(<AboutSection load={() => Promise.resolve('')} />);
    expect(screen.getByRole('heading', { name: 'About and licences' })).toBeInTheDocument();
    expect(screen.getByText(/rawler to read camera NEF files, under the GNU Lesser/)).toBeVisible();
    // The texts are the real files, which ship with the app.
    expect(screen.getByText(/You may not distribute the software/)).toBeInTheDocument();
    expect(screen.getByText(/Independent JPEG Group/)).toBeInTheDocument();
    expect(screen.getByText(/rawler-0\.8\.0\.crate/)).toBeInTheDocument();
  });

  it('loads the licence text of every component only when it is opened', async () => {
    const user = userEvent.setup();
    const load = vi.fn(() => Promise.resolve('MIT License\n\nPackages: serde 1.0.0'));
    render(<AboutSection load={load} />);
    expect(load).not.toHaveBeenCalled();
    await user.click(screen.getByText('The licence text of every component'));
    expect(await screen.findByText(/Packages: serde 1\.0\.0/)).toBeInTheDocument();
    expect(load).toHaveBeenCalledTimes(1);

    // Closing and opening again does not load it again.
    await user.click(screen.getByText('The licence text of every component'));
    await user.click(screen.getByText('The licence text of every component'));
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('says why the component licences could not be shown, and tries again when reopened', async () => {
    const user = userEvent.setup();
    const load = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(new Error('chunk failed'))
      .mockResolvedValueOnce('Apache License');
    render(<AboutSection load={load} />);
    const summary = screen.getByText('The licence text of every component');
    await user.click(summary);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The licence texts could not be loaded: chunk failed',
    );
    await user.click(summary);
    await user.click(summary);
    await waitFor(() => expect(screen.getByText('Apache License')).toBeInTheDocument());
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('has no accessibility violations', async () => {
    const { container } = render(<AboutSection load={() => Promise.resolve('text')} />);
    await expectNoA11yViolations(container);
  });
});
