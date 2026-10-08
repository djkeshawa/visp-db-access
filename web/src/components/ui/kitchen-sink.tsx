import { useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import {
  Badge,
  Button,
  ChipInput,
  Confirm,
  Empty,
  EnvBadge,
  ErrorPanel,
  Field,
  HealthBadge,
  Modal,
  Picker,
  Skeleton,
  TabBar,
  Tip,
  SwitchRow,
  SegmentedControl,
} from '.';
import { useToast } from './toast';
import { BrandMark } from './brand';
export function KitchenSink() {
  const [params] = useSearchParams(),
    [enabled, setEnabled] = useState(true),
    [modal, setModal] = useState(false),
    [confirm, setConfirm] = useState(false),
    [picker, setPicker] = useState('one'),
    [tab, setTab] = useState('one'),
    [chips, setChips] = useState(['10.0.0.0/8']);
  const toast = useToast();
  if (!params.has('kitchen-sink'))
    return (
      <Empty
        title="Preview unavailable"
        description="Enable the kitchen-sink query flag in a development build."
      />
    );
  return (
    <>
      <div className="page-heading">
        <div>
          <h1>Interface primitives</h1>
          <p>Development preview · admin only</p>
        </div>
        <BrandMark />
      </div>
      <section className="settings-section">
        <h2>Actions and feedback</h2>
        <div className="inline wrap">
          {(['primary', 'secondary', 'danger', 'ghost'] as const).map(
            (variant) => (
              <div key={variant}>
                <Button
                  variant={variant}
                  onClick={() => toast('Action completed')}
                >
                  {variant}
                </Button>
                <Button variant={variant} disabled>
                  Disabled
                </Button>
              </div>
            ),
          )}
          <Button disabled>Saving…</Button>
          <Tip text="Keyboard: ⌘ / Ctrl K">
            <Button>Tooltip</Button>
          </Tip>
        </div>
        <div className="inline wrap">
          {['neutral', 'success', 'warning', 'danger', 'info'].map((tone) => (
            <Badge key={tone} tone={tone}>
              {tone}
            </Badge>
          ))}
          <EnvBadge environment="production" />
          {['healthy', 'degraded', 'down', 'unknown'].map((status) => (
            <HealthBadge key={status} status={status} />
          ))}
        </div>
        <Field label="Name" hint="Supporting text.">
          <input placeholder="Placeholder" />
        </Field>
        <Field label="Disabled">
          <input disabled value="Read-only state" readOnly />
        </Field>
        <Field label="Invalid">
          <input aria-invalid="true" defaultValue="Invalid value" />
        </Field>
        <Field label="Notes">
          <textarea rows={2} />
        </Field>
        <Picker
          label="Choose one"
          value={picker}
          onChange={setPicker}
          options={[
            { value: 'one', label: 'One' },
            { value: 'two', label: 'Two' },
          ]}
        />
        <ChipInput label="Networks" value={chips} onChange={setChips} />
        <SwitchRow
          label="Require review"
          description="A second person reviews write queries."
          checked={enabled}
          onChange={setEnabled}
        />
        <SegmentedControl
          label="Query scope"
          value={tab}
          onChange={setTab}
          items={[
            { value: 'one', label: 'Mine' },
            { value: 'two', label: 'Everyone' },
          ]}
        />
        <label className="checkbox">
          <input type="checkbox" /> Select row
        </label>
        <TabBar
          value={tab}
          onChange={setTab}
          items={[
            { value: 'one', label: 'First' },
            { value: 'two', label: 'Second' },
          ]}
        >
          <p>Current view: {tab}</p>
        </TabBar>
        <Button onClick={() => setModal(true)}>Open dialog</Button>
        <Button onClick={() => setConfirm(true)}>Confirm action</Button>
        <Modal
          open={modal}
          onOpenChange={setModal}
          title="Dialog"
          description="Focus is contained and restored."
        >
          <p>Dialog content</p>
        </Modal>
        <Confirm
          open={confirm}
          onOpenChange={setConfirm}
          title="Confirm change?"
          description="Review before saving."
          onConfirm={() => setConfirm(false)}
        />
      </section>
      <Empty
        title="No items yet"
        description="An empty state should offer the next step."
        action={<Button onClick={() => toast('Created')}>Create item</Button>}
      />
      <Skeleton />
      <ErrorPanel
        error={new Error('The gateway is unavailable.')}
        retry={() => toast('Retry requested')}
      />
      <div className="callout warning">Warning message</div>
      <div className="callout success">Success message</div>
    </>
  );
}
