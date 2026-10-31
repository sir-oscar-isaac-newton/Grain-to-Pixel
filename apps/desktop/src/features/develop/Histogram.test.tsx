import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Histogram, histogramPath, histogramPeak, type HistogramData } from '@gtp/ui';
import { expectNoA11yViolations } from '../../test-support';

const flat = (value: number) => new Array<number>(256).fill(value);
const data = (over: Partial<HistogramData> = {}): HistogramData => ({
  red: flat(10),
  green: flat(10),
  blue: flat(10),
  luma: flat(10),
  ...over,
});

describe('Histogram', () => {
  it('is one labelled picture with a caption saying what it shows', () => {
    render(
      <Histogram data={data()} label="Histogram of the picture" caption="Black clipped: 1%." />,
    );
    const picture = screen.getByRole('img', { name: 'Histogram of the picture' });
    expect(picture.querySelectorAll('path')).toHaveLength(4);
    expect(screen.getByText('Black clipped: 1%.')).toBeInTheDocument();
  });

  it('draws an empty frame while there is nothing to show', () => {
    render(<Histogram data={null} label="Histogram" />);
    const picture = screen.getByRole('img', { name: 'Histogram' });
    expect(picture.querySelectorAll('path')).toHaveLength(0);
    expect(screen.queryByRole('figure')?.querySelector('figcaption')).toBeNull();
  });

  it('draws each channel as a closed outline over 256 bins, tallest where the count is highest', () => {
    const counts = flat(0);
    counts[0] = 100;
    counts[128] = 100;
    const path = histogramPath(counts, 100);
    expect(path.startsWith('M0,100')).toBe(true);
    expect(path.endsWith('Z')).toBe(true);
    expect(path.match(/L/g)).toHaveLength(256 + 1);
    expect(path).toContain('L0,0.0');
    expect(path).toContain('L128,0.0');
    expect(path).toContain('L1,100.0');
  });

  it('keeps a bin from rising above the picture, and scales by square root', () => {
    const counts = flat(0);
    counts[10] = 1e9;
    counts[20] = 25;
    const path = histogramPath(counts, 100);
    expect(path).toContain('L10,0.0');
    // 25 of a peak of 100: sqrt(0.25) = half the height.
    expect(path).toContain('L20,50.0');
  });

  it('measures against a high percentile, so a spike of clipped pixels does not flatten the rest', () => {
    const spiky = data({ luma: Object.assign(flat(10), { 255: 1e7 }) });
    expect(histogramPeak(spiky)).toBe(10);
    expect(
      histogramPeak(data({ red: flat(0), green: flat(0), blue: flat(0), luma: flat(0) })),
    ).toBe(1);
  });

  it('has no accessibility violations', async () => {
    const { container } = render(
      <Histogram data={data()} label="Histogram" caption="Nothing clipped." />,
    );
    await expectNoA11yViolations(container);
  });
});
