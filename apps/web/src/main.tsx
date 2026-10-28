import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import './styles/globals.css';
import { QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider } from '@tanstack/react-router';
import { MotionConfig } from 'motion/react';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { router } from './app/router.tsx';
import { Toaster } from './components/ui/toaster.tsx';
import { TooltipProvider } from './components/ui/menu.tsx';
import { installClipboardFallback } from './lib/clipboard.ts';
import { PreferencesProvider } from './lib/preferences.tsx';
import { queryClient } from './lib/query-client.ts';

// Before anything renders a copy button.
installClipboardFallback();

const container = document.getElementById('root');
if (!container) {
  throw new Error('Root element #root is missing from index.html');
}

createRoot(container).render(
  <StrictMode>
    <PreferencesProvider>
      <QueryClientProvider client={queryClient}>
        <MotionConfig reducedMotion="user">
          <TooltipProvider delayDuration={400} skipDelayDuration={200}>
            <RouterProvider router={router} />
            <Toaster />
          </TooltipProvider>
        </MotionConfig>
      </QueryClientProvider>
    </PreferencesProvider>
  </StrictMode>,
);
