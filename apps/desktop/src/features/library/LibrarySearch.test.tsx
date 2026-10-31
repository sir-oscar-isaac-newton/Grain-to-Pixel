import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import { ipc, libraryDetails, makeFrames, makeRoll, resetFake, state } from './fake-ipc';
import { resetLibraryStore, useLibraryStore } from './library-store';
import { LibraryView } from './LibraryView';
import { resetThumbnails } from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

/** Rolls "Summer" (3 frames, Portra, Nikon, tag "Japan") and "Winter" (2 frames, HP5, Leica). */
function openLibrary(over = {}) {
  const summer = makeRoll({
    id: 'summer',
    name: 'Summer',
    film_stock: 'Portra 400',
    camera: 'Nikon F3',
    iso_box: 400,
  });
  const winter = makeRoll({ id: 'winter', name: 'Winter', film_stock: 'HP5', camera: 'Leica M6' });
  state.details = libraryDetails(over);
  state.rolls = [summer, winter];
  state.frames = [...makeFrames('summer', 3), ...makeFrames('winter', 2)];
  state.tags = [
    { id: 'tag-japan', name: 'Japan' },
    { id: 'tag-street', name: 'street' },
  ];
  state.rollTags = { summer: ['Japan'] };
  state.frameTags = { 'summer-f00001': ['street'], 'winter-f00001': ['street'] };
}

async function showLibrary() {
  render(<LibraryView />);
  await screen.findAllByRole('gridcell');
}

const cells = () => screen.getAllByRole('gridcell');

describe('search, filters and tags in the Library view', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    restoreLayout = mockLayout(900, 600);
  });
  afterEach(() => restoreLayout());

  describe('the search box', () => {
    it('searches after a pause in typing, with the roll when one is chosen, and counts the frames', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      expect(ipc.search).not.toHaveBeenCalled();

      await user.type(screen.getByRole('searchbox', { name: 'Search frames' }), 'leica');
      await waitFor(() => expect(cells()).toHaveLength(2));
      expect(ipc.search).toHaveBeenLastCalledWith(
        expect.objectContaining({ text: 'leica', roll_id: null, tags_all: [] }),
        0,
        200,
      );
      // Typing a letter at a time does not search a letter at a time.
      expect(ipc.search.mock.calls.length).toBeLessThan(5);
      expect(await screen.findByText('2 frames found')).toBeInTheDocument();

      await user.click(screen.getByRole('button', { name: /Winter/ }));
      await waitFor(() =>
        expect(ipc.search).toHaveBeenLastCalledWith(
          expect.objectContaining({ text: 'leica', roll_id: 'winter' }),
          0,
          200,
        ),
      );
    });

    it('says when nothing matches, and clears back to the roll', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      await user.type(screen.getByRole('searchbox'), 'zzz');
      expect(await screen.findByText(/No frames match/)).toBeInTheDocument();
      expect(screen.queryAllByRole('gridcell')).toHaveLength(0);

      await user.click(screen.getByRole('button', { name: 'Clear search' }));
      await waitFor(() => expect(cells()).toHaveLength(5));
      expect(screen.getByRole('searchbox')).toHaveValue('');
      expect(screen.getByRole('button', { name: 'Clear search' })).toBeDisabled();
    });

    it('shows what the backend refuses', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      ipc.search.mockRejectedValueOnce(
        new Error('Dates are written as 2026, 2026-07 or 2026-07-14.'),
      );
      await user.type(screen.getByRole('searchbox'), 'x');
      expect(await screen.findByRole('alert')).toHaveTextContent('Dates are written as');
    });
  });

  describe('filters', () => {
    it('are folded away until asked for, and say how many are set', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      const toggle = screen.getByRole('button', { name: 'Filters' });
      expect(toggle).toHaveAttribute('aria-expanded', 'false');
      expect(screen.queryByRole('combobox', { name: 'Film' })).not.toBeInTheDocument();

      await user.click(toggle);
      expect(toggle).toHaveAttribute('aria-expanded', 'true');
      await user.type(screen.getByRole('combobox', { name: 'Film' }), 'HP5');
      expect(await screen.findByRole('button', { name: 'Filters (1)' })).toBeInTheDocument();
      await waitFor(() => expect(cells()).toHaveLength(2));
      expect(ipc.search).toHaveBeenLastCalledWith(
        expect.objectContaining({ film_stock: 'HP5' }),
        0,
        200,
      );
    });

    it('filter by camera, ISO and the shooting dates, and suggest earlier film and cameras', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      await user.click(screen.getByRole('button', { name: 'Filters' }));
      const options = (name: string) => {
        const input = screen.getByRole('combobox', { name });
        const list = document.getElementById(input.getAttribute('list') ?? '');
        return [...(list?.querySelectorAll('option') ?? [])].map((o) => o.getAttribute('value'));
      };
      expect(options('Film')).toEqual(['HP5', 'Portra 400']);
      expect(options('Camera')).toEqual(['Leica M6', 'Nikon F3']);

      await user.type(screen.getByRole('combobox', { name: 'Camera' }), 'Nikon F3');
      await user.type(screen.getByRole('textbox', { name: 'ISO' }), '4x00');
      await user.type(screen.getByRole('textbox', { name: 'Shot from' }), '2026-07');
      await user.type(screen.getByRole('textbox', { name: 'Shot to' }), '2026-08-02');
      expect(screen.getByRole('textbox', { name: 'ISO' })).toHaveValue('400');
      await waitFor(() => expect(cells()).toHaveLength(3));
      expect(ipc.search).toHaveBeenLastCalledWith(
        {
          text: null,
          tags_all: [],
          roll_id: null,
          camera: 'Nikon F3',
          film_stock: null,
          iso: 400,
          date_from: '2026-07',
          date_to: '2026-08-02',
        },
        0,
        200,
      );
      expect(screen.getByRole('button', { name: 'Filters (4)' })).toBeInTheDocument();
    });

    it('match tags, which can be added by name and removed again', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      await user.click(screen.getByRole('button', { name: 'Filters' }));
      const tags = screen.getByRole('group', { name: 'Tags to match' });
      expect(within(tags).getByText(/No tags chosen/)).toBeInTheDocument();

      // Found by name, whatever the case; the roll's tag reaches its frames.
      await user.type(within(tags).getByRole('combobox', { name: 'Tag to match' }), 'japan{Enter}');
      await waitFor(() => expect(cells()).toHaveLength(3));
      expect(within(tags).getByText('Japan')).toBeInTheDocument();
      expect(ipc.search).toHaveBeenLastCalledWith(
        expect.objectContaining({ tags_all: ['tag-japan'] }),
        0,
        200,
      );

      await user.type(
        within(tags).getByRole('combobox', { name: 'Tag to match' }),
        'street{Enter}',
      );
      await waitFor(() => expect(cells()).toHaveLength(1));

      await user.click(within(tags).getByRole('button', { name: 'Stop matching Japan' }));
      await waitFor(() => expect(cells()).toHaveLength(2));
      expect(useLibraryStore.getState().filters.tagIds).toEqual(['tag-street']);
    });

    it('say so when the tag does not exist', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      await user.click(screen.getByRole('button', { name: 'Filters' }));
      await user.type(screen.getByRole('combobox', { name: 'Tag to match' }), 'nowhere{Enter}');
      expect(
        await screen.findByText('There is no tag called “nowhere” in this library.'),
      ).toBeInTheDocument();
      expect(useLibraryStore.getState().filters.tagIds).toEqual([]);
      expect(ipc.search).not.toHaveBeenCalled();
    });
  });

  describe('tagging the selected frames', () => {
    it('appears with a selection and adds a tag to every selected frame', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      expect(screen.queryByRole('group', { name: 'Tags on the selected frames' })).toBeNull();

      await user.click(cells()[0] as HTMLElement);
      await user.keyboard('{Control>}');
      await user.click(cells()[1] as HTMLElement);
      await user.keyboard('{/Control}');
      const group = await screen.findByRole('group', { name: 'Tags on the selected frames' });
      expect(screen.getByText('Tags for 2 selected frames')).toBeInTheDocument();
      // Own tags only: "street" is on the first frame; the roll's "Japan" is not shown here.
      expect(await within(group).findByText('street')).toBeInTheDocument();
      expect(within(group).queryByText('Japan')).not.toBeInTheDocument();

      await user.type(
        within(group).getByRole('combobox', { name: 'Tag the selected frames' }),
        'Night market{Enter}',
      );
      await waitFor(() => expect(ipc.tagApply).toHaveBeenCalledTimes(1));
      expect(ipc.tagApply).toHaveBeenCalledWith(
        ['summer-f00001', 'summer-f00002'],
        ['Night market'],
        [],
      );
      expect(await within(group).findByText('Night market')).toBeInTheDocument();
      expect(state.tags.map((t) => t.name)).toContain('Night market');
    });

    it('removes a tag from the selected frames', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      await user.click(cells()[0] as HTMLElement);
      const group = await screen.findByRole('group', { name: 'Tags on the selected frames' });
      await user.click(
        await within(group).findByRole('button', {
          name: 'Remove street from the selected frames',
        }),
      );
      await waitFor(() =>
        expect(ipc.tagApply).toHaveBeenCalledWith(['summer-f00001'], [], ['street']),
      );
      await waitFor(() =>
        expect(within(group).getByText('No tags of their own yet.')).toBeInTheDocument(),
      );
    });

    it('re-runs a tag search so the grid follows the change', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      await user.click(screen.getByRole('button', { name: 'Filters' }));
      await user.type(screen.getByRole('combobox', { name: 'Tag to match' }), 'street{Enter}');
      await waitFor(() => expect(cells()).toHaveLength(2));

      await user.click(cells()[0] as HTMLElement);
      const group = await screen.findByRole('group', { name: 'Tags on the selected frames' });
      await user.click(
        await within(group).findByRole('button', {
          name: 'Remove street from the selected frames',
        }),
      );
      await waitFor(() => expect(cells()).toHaveLength(1));
    });

    it('reports a failure instead of losing it', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      await user.click(cells()[0] as HTMLElement);
      const group = await screen.findByRole('group', { name: 'Tags on the selected frames' });
      ipc.tagApply.mockRejectedValueOnce(new Error('Tag names are at most 64 characters.'));
      await user.type(
        within(group).getByRole('combobox', { name: 'Tag the selected frames' }),
        'x{Enter}',
      );
      expect(
        await screen.findByText(
          'The tags could not be changed: Tag names are at most 64 characters.',
        ),
      ).toBeInTheDocument();
    });

    it('cannot change a read-only library', async () => {
      const user = userEvent.setup();
      openLibrary({ read_only: true });
      await showLibrary();
      await user.click(cells()[0] as HTMLElement);
      const group = await screen.findByRole('group', { name: 'Tags on the selected frames' });
      expect(
        within(group).getByRole('combobox', { name: 'Tag the selected frames' }),
      ).toBeDisabled();
      expect(
        await within(group).findByRole('button', {
          name: 'Remove street from the selected frames',
        }),
      ).toBeDisabled();
    });
  });

  describe('roll tags in the roll editor', () => {
    it('start from the roll’s tags and save the difference with the roll', async () => {
      const user = userEvent.setup();
      openLibrary();
      state.rollTags = { summer: ['Japan', 'Trip'] };
      await showLibrary();
      await user.click(screen.getByRole('button', { name: /Summer/ }));
      await user.click(screen.getByRole('button', { name: 'Edit roll' }));
      const dialog = await screen.findByRole('dialog', { name: 'Edit roll' });
      const tags = await within(dialog).findByRole('group', { name: 'Roll tags' });
      expect(await within(tags).findByText('Trip')).toBeInTheDocument();

      await user.click(within(tags).getByRole('button', { name: 'Remove Japan from the roll' }));
      await user.type(
        within(tags).getByRole('combobox', { name: 'Add a tag to the roll' }),
        'Autumn{Enter}',
      );
      // The same tag in another case is the same tag.
      await user.type(
        within(tags).getByRole('combobox', { name: 'Add a tag to the roll' }),
        'autumn{Enter}',
      );
      expect(ipc.tagApplyRoll).not.toHaveBeenCalled();
      await user.click(within(dialog).getByRole('button', { name: 'Save' }));

      await waitFor(() => expect(ipc.tagApplyRoll).toHaveBeenCalledTimes(1));
      expect(ipc.tagApplyRoll).toHaveBeenCalledWith('summer', ['Autumn'], ['Japan']);
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(state.rollTags.summer).toEqual(['Trip', 'Autumn']);
    });

    it('can be given to a new roll, and nothing is sent when they did not change', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      await user.click(screen.getByRole('button', { name: 'New roll' }));
      let dialog = await screen.findByRole('dialog', { name: 'New roll' });
      await user.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'Spring');
      await user.type(
        within(dialog).getByRole('combobox', { name: 'Add a tag to the roll' }),
        'Blossom{Enter}',
      );
      await user.click(within(dialog).getByRole('button', { name: 'Save' }));
      await waitFor(() => expect(ipc.tagApplyRoll).toHaveBeenCalledTimes(1));
      expect(ipc.tagApplyRoll).toHaveBeenCalledWith(expect.any(String), ['Blossom'], []);

      // Editing without touching the tags sends no tag change.
      ipc.tagApplyRoll.mockClear();
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      await user.click(screen.getByRole('button', { name: 'Edit roll' }));
      dialog = await screen.findByRole('dialog', { name: 'Edit roll' });
      await within(dialog).findByText('Blossom');
      await user.click(within(dialog).getByRole('button', { name: 'Save' }));
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(ipc.tagApplyRoll).not.toHaveBeenCalled();
    });
  });

  describe('accessibility', () => {
    it('has no violations with the filters open and frames selected', async () => {
      const user = userEvent.setup();
      openLibrary();
      const { container } = render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: 'Filters' }));
      await user.type(screen.getByRole('combobox', { name: 'Tag to match' }), 'japan{Enter}');
      await waitFor(() => expect(cells()).toHaveLength(3));
      await user.click(cells()[0] as HTMLElement);
      await screen.findByRole('group', { name: 'Tags on the selected frames' });
      await expectNoA11yViolations(container);
    });

    it('has no violations in the roll editor with tags', async () => {
      const user = userEvent.setup();
      openLibrary();
      await showLibrary();
      await user.click(screen.getByRole('button', { name: /Summer/ }));
      await user.click(screen.getByRole('button', { name: 'Edit roll' }));
      const dialog = await screen.findByRole('dialog', { name: 'Edit roll' });
      await within(dialog).findByText('Japan');
      await expectNoA11yViolations(dialog);
    });
  });
});
