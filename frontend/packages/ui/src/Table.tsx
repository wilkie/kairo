// Column-driven thin wrapper around MUI Table primitives. We
// keep the `columns: TableColumn<T>[]` API because declarative
// columns + `rows` are far less verbose than hand-composing
// MUI's TableHead / TableBody / TableRow / TableCell trees per
// call site. Under the hood we render the same MUI primitives
// so theming, density, and aria are all native.

import type { ReactNode } from 'react';
import MuiTable from '@mui/material/Table';
import TableBody from '@mui/material/TableBody';
import TableCell from '@mui/material/TableCell';
import TableContainer from '@mui/material/TableContainer';
import TableHead from '@mui/material/TableHead';
import TableRow from '@mui/material/TableRow';

export interface TableColumn<T> {
  key: string;
  /** Column header label. */
  header: ReactNode;
  /** Cell renderer for one row. */
  cell: (row: T) => ReactNode;
}

export interface TableProps<T> {
  columns: ReadonlyArray<TableColumn<T>>;
  rows: ReadonlyArray<T>;
  /** Function returning a stable key per row. */
  rowKey: (row: T, index: number) => string;
  /** Caption text for screen readers (and optionally visible). */
  caption?: ReactNode;
  /** Rendered when `rows` is empty. */
  emptyState?: ReactNode;
}

export function Table<T>({ columns, rows, rowKey, caption, emptyState }: TableProps<T>) {
  if (rows.length === 0 && emptyState !== undefined) {
    return <>{emptyState}</>;
  }
  return (
    <TableContainer>
      <MuiTable>
        {caption !== undefined && <caption>{caption}</caption>}
        <TableHead>
          <TableRow>
            {columns.map((col) => (
              <TableCell key={col.key} scope="col">
                {col.header}
              </TableCell>
            ))}
          </TableRow>
        </TableHead>
        <TableBody>
          {rows.map((row, idx) => (
            <TableRow key={rowKey(row, idx)}>
              {columns.map((col) => (
                <TableCell key={col.key}>{col.cell(row)}</TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </MuiTable>
    </TableContainer>
  );
}
