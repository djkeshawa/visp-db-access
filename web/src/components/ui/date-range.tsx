import { useState } from 'react';
import * as Popover from '@radix-ui/react-popover';
import { CalendarDays, Check } from 'lucide-react';
import { Button, Field } from '.';
import { datePreset } from '../../lib/activity';
export function DateRange({
  from,
  to,
  onFrom,
  onTo,
  onChange,
}: {
  from: string;
  to: string;
  onFrom: (value: string) => void;
  onTo: (value: string) => void;
  onChange: (range: { from: string; to: string }) => void;
}) {
  const [open, setOpen] = useState(false),
    [custom, setCustom] = useState(false);
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <Button aria-label="Date range">
          <CalendarDays size={16} />
          {from || to ? `${from || 'Start'} – ${to || 'Now'}` : 'Date range'}
        </Button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content className="popover date-range-popover" align="end">
          <div className="date-range-presets">
            {[
              { days: 1, label: 'Today' },
              { days: 7, label: 'Last 7 days' },
              { days: 30, label: 'Last 30 days' },
            ].map((item) => {
              const range = datePreset(item.days);
              return (
                <Button
                  variant="ghost"
                  key={item.days}
                  onClick={() => {
                    onChange(range);
                    setOpen(false);
                  }}
                >
                  {item.label}
                  {from === range.from && to === range.to && (
                    <Check size={14} />
                  )}
                </Button>
              );
            })}
            <Button variant="ghost" onClick={() => setCustom(true)}>
              Custom{custom && <Check size={14} />}
            </Button>
            <Button
              variant="ghost"
              onClick={() => {
                onChange({ from: '', to: '' });
                setOpen(false);
              }}
            >
              All loaded dates
            </Button>
          </div>
          {custom && (
            <div className="date-custom">
              <Field label="From date">
                <input
                  type="date"
                  value={from}
                  onChange={(event) => onFrom(event.target.value)}
                  max={to || undefined}
                />
              </Field>
              <Field label="Through date">
                <input
                  type="date"
                  value={to}
                  onChange={(event) => onTo(event.target.value)}
                  min={from || undefined}
                />
              </Field>
              <p className="muted">UTC dates · loaded records only</p>
              <Button onClick={() => setOpen(false)}>Done</Button>
            </div>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
/** Inclusive UTC calendar-day filtering for loaded records; server cursors are untouched. */
export function inDateRange(value: string, from: string, to: string): boolean {
  const day = value.slice(0, 10);
  return (!from || day >= from) && (!to || day <= to);
}
