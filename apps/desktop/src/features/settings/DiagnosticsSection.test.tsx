import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../test-support';
import { ipc, resetFake, state } from '../library/fake-ipc';
import { DiagnosticsSection } from './DiagnosticsSection';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

const exportButton = () => screen.getByRole('button', { name: /Export diagnostics|Saving/ });

describe('DiagnosticsSection', () => {
  beforeEach(() => resetFake());

  it('shows the counters of the open library, with "Not yet" for what has not happened', async () => {
    state.counters = {
      first_import_at: '2026-10-30T10:00:00Z',
      first_convert_at: null,
      first_export_at: null,
      frames_imported: 36,
      frames_exported: 4,
      scans_failed: 1,
    };
    render(<DiagnosticsSection />);
    expect(await screen.findByText('36')).toBeInTheDocument();
    expect(screen.getByText('Frames exported').nextElementSibling).toHaveTextContent('4');
    expect(screen.getByText('Scans that failed').nextElementSibling).toHaveTextContent('1');
    expect(screen.getByText('First import').nextElementSibling).not.toHaveTextContent('Not yet');
    expect(screen.getByText('First edit').nextElementSibling).toHaveTextContent('Not yet');
    expect(screen.getByText('First export').nextElementSibling).toHaveTextContent('Not yet');
  });

  it('says to open a library when there are no counters, and still offers the export', async () => {
    render(<DiagnosticsSection />);
    expect(await screen.findByText('Open a library to see its counters.')).toBeInTheDocument();
    expect(exportButton()).toBeEnabled();
  });

  it('says why the counters could not be read', async () => {
    ipc.countersGet.mockRejectedValueOnce(new Error('The library is damaged.'));
    render(<DiagnosticsSection />);
    expect(
      await screen.findByText('Could not read the counters: The library is damaged.'),
    ).toBeInTheDocument();
  });

  it('saves the diagnostics where the user chose and says where', async () => {
    const user = userEvent.setup();
    render(<DiagnosticsSection />);
    await user.click(exportButton());
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Saved the diagnostics to /home/film/diagnostics.zip.',
    );
    expect(state.diagnosticsExports).toEqual(['/home/film/diagnostics.zip']);
    expect(exportButton()).toBeEnabled();
  });

  it('does nothing when the save dialog is cancelled', async () => {
    state.diagnosticsPick = null;
    const user = userEvent.setup();
    render(<DiagnosticsSection />);
    await user.click(exportButton());
    expect(ipc.pickDiagnosticsFile).toHaveBeenCalledTimes(1);
    expect(ipc.diagnosticsExport).not.toHaveBeenCalled();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('says why the diagnostics could not be saved', async () => {
    state.diagnosticsFails = 'The disk is full.';
    const user = userEvent.setup();
    render(<DiagnosticsSection />);
    await user.click(exportButton());
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not save the diagnostics: The disk is full.',
    );
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(exportButton()).toBeEnabled();
  });

  it('has no accessibility violations', async () => {
    state.counters = {
      first_import_at: null,
      first_convert_at: null,
      first_export_at: null,
      frames_imported: 0,
      frames_exported: 0,
      scans_failed: 0,
    };
    const { container } = render(<DiagnosticsSection />);
    await screen.findByText('Frames imported');
    await expectNoA11yViolations(container);
  });
});
