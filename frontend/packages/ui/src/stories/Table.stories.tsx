import type { Meta, StoryObj } from '@storybook/react';
import { Table, type TableColumn } from '../Table';

interface BranchRow {
  actor: string;
  name: string;
  statement_id: string;
  created_at: string;
}

const rows: BranchRow[] = [
  {
    actor: 'kairo:actor:zMockActor1',
    name: 'head',
    statement_id: 'kairo:stmt:zMockStmt1',
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    actor: 'kairo:actor:zMockActor1',
    name: 'experimental',
    statement_id: 'kairo:stmt:zMockStmt2',
    created_at: '2026-01-02T00:00:00Z',
  },
];

const columns: TableColumn<BranchRow>[] = [
  { key: 'name', header: 'Name', cell: (r) => r.name },
  {
    key: 'actor',
    header: 'Actor',
    cell: (r) => <code>{r.actor}</code>,
  },
  {
    key: 'statement',
    header: 'Statement',
    cell: (r) => <code>{r.statement_id}</code>,
  },
  { key: 'created_at', header: 'Created', cell: (r) => r.created_at },
];

const meta: Meta<typeof Table<BranchRow>> = {
  title: 'Primitives/Table',
  component: Table,
};
export default meta;

type Story = StoryObj<typeof Table<BranchRow>>;

export const Populated: Story = {
  args: {
    columns,
    rows,
    rowKey: (r) => r.statement_id,
    caption: 'Branch tips',
  },
};

export const Empty: Story = {
  args: {
    columns,
    rows: [],
    rowKey: (r) => r.statement_id,
    emptyState: <p style={{ padding: '1rem' }}>No branches.</p>,
  },
};
