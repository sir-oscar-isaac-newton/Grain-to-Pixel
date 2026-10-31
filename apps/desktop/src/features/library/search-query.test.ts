import { describe, expect, it } from 'vitest';
import { NO_FILTERS, activeFilterCount, isFiltered, toQuery } from './search-query';

describe('search-query', () => {
  it('treats empty and blank filters as no filter', () => {
    expect(isFiltered(NO_FILTERS)).toBe(false);
    expect(isFiltered({ ...NO_FILTERS, text: '   ', camera: ' ' })).toBe(false);
    expect(activeFilterCount(NO_FILTERS)).toBe(0);
  });

  it('notices each kind of filter', () => {
    expect(isFiltered({ ...NO_FILTERS, text: 'tokyo' })).toBe(true);
    expect(isFiltered({ ...NO_FILTERS, tagIds: ['t1'] })).toBe(true);
    expect(isFiltered({ ...NO_FILTERS, iso: '400' })).toBe(true);
    expect(isFiltered({ ...NO_FILTERS, from: '2026' })).toBe(true);
  });

  it('counts the filters under the search box, not the text', () => {
    const filters = { ...NO_FILTERS, text: 'x', film: 'Portra', iso: '400', tagIds: ['a', 'b'] };
    expect(activeFilterCount(filters)).toBe(3);
  });

  it('builds the backend query, trimming and leaving unset parts null', () => {
    expect(toQuery(NO_FILTERS, null)).toEqual({
      text: null,
      tags_all: [],
      roll_id: null,
      date_from: null,
      date_to: null,
      film_stock: null,
      camera: null,
      iso: null,
    });
    expect(
      toQuery(
        {
          text: ' night ',
          tagIds: ['t1'],
          film: ' Portra 400',
          camera: 'Nikon F3 ',
          iso: '400',
          from: '2026-07',
          to: '2026-08-02',
        },
        'roll-1',
      ),
    ).toEqual({
      text: 'night',
      tags_all: ['t1'],
      roll_id: 'roll-1',
      date_from: '2026-07',
      date_to: '2026-08-02',
      film_stock: 'Portra 400',
      camera: 'Nikon F3',
      iso: 400,
    });
  });

  it('drops an ISO that is not a number above 0', () => {
    expect(toQuery({ ...NO_FILTERS, iso: '0' }, null).iso).toBeNull();
    expect(toQuery({ ...NO_FILTERS, iso: '4x' }, null).iso).toBeNull();
  });
});
