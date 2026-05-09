// Wrapper around MUI Tabs/Tab. We keep the
// `tabs: TabsTab[]` API so call sites declare the active panel
// alongside the trigger; MUI's primitives need separate JSX
// for each. ARIA wiring + keyboard navigation are MUI-native.

import { useId, useState, type ReactNode } from 'react';
import Box from '@mui/material/Box';
import MuiTab from '@mui/material/Tab';
import MuiTabs from '@mui/material/Tabs';

export interface TabsTab {
  /** Stable id used for active-tab tracking + ARIA wiring. */
  id: string;
  label: ReactNode;
  content: ReactNode;
}

export interface TabsProps {
  tabs: ReadonlyArray<TabsTab>;
  /** Tab to render initially. Defaults to the first. */
  defaultTab?: string;
  /** Optional controlled-mode: caller manages active tab. */
  activeTab?: string;
  onActiveTabChange?: (id: string) => void;
}

export function Tabs({ tabs, defaultTab, activeTab, onActiveTabChange }: TabsProps) {
  const generatedId = useId();
  const fallback = defaultTab ?? tabs[0]?.id ?? '';
  const [internal, setInternal] = useState<string>(fallback);
  const current = activeTab ?? internal;

  const handleChange = (_event: React.SyntheticEvent, newValue: string) => {
    if (onActiveTabChange) onActiveTabChange(newValue);
    if (activeTab === undefined) setInternal(newValue);
  };

  const activeTabRecord = tabs.find((tab) => tab.id === current) ?? tabs[0];

  return (
    <Box>
      <Box sx={{ borderBottom: 1, borderColor: 'divider' }}>
        <MuiTabs value={current} onChange={handleChange} aria-label="Tabs">
          {tabs.map((tab) => (
            <MuiTab
              key={tab.id}
              value={tab.id}
              label={tab.label}
              id={`${generatedId}-trigger-${tab.id}`}
              aria-controls={`${generatedId}-panel-${tab.id}`}
            />
          ))}
        </MuiTabs>
      </Box>
      {activeTabRecord !== undefined && (
        <Box
          role="tabpanel"
          id={`${generatedId}-panel-${activeTabRecord.id}`}
          aria-labelledby={`${generatedId}-trigger-${activeTabRecord.id}`}
          sx={{ pt: 2 }}
        >
          {activeTabRecord.content}
        </Box>
      )}
    </Box>
  );
}
