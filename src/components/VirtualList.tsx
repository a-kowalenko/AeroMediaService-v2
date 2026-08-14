import { useMemo, useRef, useState, useEffect, type ReactNode, type UIEvent } from "react";

type Props<T> = {
  items: T[];
  rowHeight: number;
  /** Viewport height in px (scroll container). */
  height: number;
  overscan?: number;
  getKey: (item: T, index: number) => string;
  renderRow: (item: T, index: number) => ReactNode;
  empty?: ReactNode;
  className?: string;
};

/**
 * Simple fixed-row virtualizer for large history pages (keeps parent pagination).
 */
export function VirtualList<T>({
  items,
  rowHeight,
  height,
  overscan = 6,
  getKey,
  renderRow,
  empty,
  className,
}: Props<T>) {
  const scrollerRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);

  useEffect(() => {
    setScrollTop(0);
    if (scrollerRef.current) scrollerRef.current.scrollTop = 0;
  }, [items]);

  const { start, end, offsetY, totalHeight } = useMemo(() => {
    const total = items.length * rowHeight;
    const startIdx = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
    const visible = Math.ceil(height / rowHeight) + overscan * 2;
    const endIdx = Math.min(items.length, startIdx + visible);
    return {
      start: startIdx,
      end: endIdx,
      offsetY: startIdx * rowHeight,
      totalHeight: total,
    };
  }, [items.length, rowHeight, height, overscan, scrollTop]);

  if (items.length === 0) {
    return <div className={className}>{empty}</div>;
  }

  function onScroll(e: UIEvent<HTMLDivElement>) {
    setScrollTop(e.currentTarget.scrollTop);
  }

  const slice = items.slice(start, end);

  return (
    <div
      ref={scrollerRef}
      className={className}
      style={{ height, overflow: "auto", position: "relative" }}
      onScroll={onScroll}
    >
      <div style={{ height: totalHeight, position: "relative" }}>
        <div style={{ transform: `translateY(${offsetY}px)` }}>
          {slice.map((item, i) => {
            const index = start + i;
            return (
              <div key={getKey(item, index)} style={{ height: rowHeight }}>
                {renderRow(item, index)}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
