import styles from './Histogram.module.css';

/** 256 counts per channel (index 0 is black, 255 is white). */
export interface HistogramData {
  red: ArrayLike<number>;
  green: ArrayLike<number>;
  blue: ArrayLike<number>;
  luma: ArrayLike<number>;
}

interface HistogramProps {
  /** The counts to draw; null draws an empty frame (nothing rendered yet). */
  data: HistogramData | null;
  /** Names the picture for assistive technology, for example "Histogram of the preview". */
  label: string;
  /** What the picture says in words (for example how much is clipped), shown beneath it. */
  caption?: string;
}

const BINS = 256;
const HEIGHT = 100;

/**
 * The height every bin is measured against: a high percentile of all the counts, so a spike at
 * pure black or pure white (clipped pixels) does not flatten the rest of the picture.
 */
export function histogramPeak(data: HistogramData): number {
  const all: number[] = [];
  for (const channel of [data.red, data.green, data.blue, data.luma]) {
    for (let i = 0; i < BINS; i += 1) all.push(channel[i] ?? 0);
  }
  all.sort((a, b) => a - b);
  return Math.max(1, all[Math.floor(all.length * 0.995)] ?? 1);
}

/** The outline of one channel as an SVG path, heights on a square-root scale. */
export function histogramPath(counts: ArrayLike<number>, peak: number): string {
  const points: string[] = [`M0,${HEIGHT}`];
  for (let i = 0; i < BINS; i += 1) {
    const share = Math.min(1, Math.sqrt((counts[i] ?? 0) / peak));
    points.push(`L${i},${(HEIGHT - share * HEIGHT).toFixed(1)}`);
  }
  points.push(`L${BINS - 1},${HEIGHT}`, 'Z');
  return points.join(' ');
}

/**
 * Luma and red, green and blue histograms of an image, as one picture. The meaning is in the label
 * and caption; the colours only tell the channels apart.
 */
export function Histogram({ data, label, caption }: HistogramProps) {
  const peak = data ? histogramPeak(data) : 1;
  return (
    <figure className={styles.figure}>
      <svg
        role="img"
        aria-label={label}
        viewBox={`0 0 ${BINS} ${HEIGHT}`}
        preserveAspectRatio="none"
        className={styles.picture}
      >
        {data && (
          <>
            <path d={histogramPath(data.luma, peak)} className={styles.luma} />
            <path d={histogramPath(data.red, peak)} className={styles.red} />
            <path d={histogramPath(data.green, peak)} className={styles.green} />
            <path d={histogramPath(data.blue, peak)} className={styles.blue} />
          </>
        )}
      </svg>
      {caption && <figcaption className={styles.caption}>{caption}</figcaption>}
    </figure>
  );
}
