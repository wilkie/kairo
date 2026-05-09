import type { Meta, StoryObj } from '@storybook/react';
import { useState } from 'react';
import { Dialog } from '../Dialog';
import { Button } from '../Button';

const meta: Meta<typeof Dialog> = {
  title: 'Primitives/Dialog',
  component: Dialog,
};
export default meta;

type Story = StoryObj<typeof Dialog>;

function DialogDemo() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="primary" onClick={() => setOpen(true)}>
        Open dialog
      </Button>
      <Dialog
        open={open}
        onClose={() => setOpen(false)}
        title="Confirm action"
        footer={
          <>
            <Button onClick={() => setOpen(false)}>Cancel</Button>
            <Button variant="primary" onClick={() => setOpen(false)}>
              Confirm
            </Button>
          </>
        }
      >
        <p>Native &lt;dialog&gt; element with focus trap, ESC-to-close, and a backdrop.</p>
      </Dialog>
    </>
  );
}

export const Default: Story = {
  render: () => <DialogDemo />,
};
