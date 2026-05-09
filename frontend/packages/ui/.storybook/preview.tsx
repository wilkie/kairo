import type { Preview } from '@storybook/react';
import { KairoUiProvider } from '../src/KairoUiProvider';

const preview: Preview = {
  parameters: {
    controls: {
      matchers: {
        color: /(background|color)$/i,
        date: /Date$/i,
      },
    },
  },
  decorators: [
    (Story) => (
      <KairoUiProvider>
        <Story />
      </KairoUiProvider>
    ),
  ],
};

export default preview;
