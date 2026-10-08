import { useRef } from 'react';
/** Pointer and keyboard resizing share a bounded range. */
export function SplitHandle({
  value,
  onChange,
  minimum = 120,
  maximum = 640,
}: {
  value: number;
  onChange: (value: number) => void;
  minimum?: number;
  maximum?: number;
}) {
  const drag = useRef<{ y: number; value: number } | null>(null);
  const update = (value: number) =>
    onChange(Math.min(maximum, Math.max(minimum, value)));
  return (
    <div
      className="split-handle"
      role="separator"
      aria-label="Resize editor and results"
      aria-orientation="horizontal"
      aria-valuemin={minimum}
      aria-valuemax={maximum}
      aria-valuenow={value}
      tabIndex={0}
      onKeyDown={(event) => {
        if (['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {
          event.preventDefault();
          update(
            event.key === 'Home'
              ? minimum
              : event.key === 'End'
                ? maximum
                : value + (event.key === 'ArrowDown' ? 16 : -16),
          );
        }
      }}
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { y: event.clientY, value };
      }}
      onPointerMove={(event) => {
        if (drag.current)
          update(drag.current.value + event.clientY - drag.current.y);
      }}
      onPointerUp={() => {
        drag.current = null;
      }}
      onLostPointerCapture={() => {
        drag.current = null;
      }}
    >
      <span />
    </div>
  );
}
