import React from 'react';
import * as Sentry from '@sentry/react';
import posthog from 'posthog-js';
import {
  createRoutesFromChildren,
  matchRoutes,
  useLocation,
  useNavigationType,
} from 'react-router-dom';

let sentryInitialized = false;
let posthogInitialized = false;
let telemetryEnabled = false;

const sentryDsn = import.meta.env.VITE_SENTRY_DSN?.trim();
const posthogApiKey = import.meta.env.VITE_POSTHOG_API_KEY?.trim();
const posthogApiEndpoint = import.meta.env.VITE_POSTHOG_API_ENDPOINT?.trim();

export function configureFrontendTelemetry(
  analyticsEnabled: boolean,
  analyticsUserId: string | null
) {
  telemetryEnabled = analyticsEnabled;

  if (!analyticsEnabled) {
    if (posthogInitialized) {
      posthog.opt_out_capturing();
      posthog.reset();
    }
    if (sentryInitialized) {
      Sentry.setUser(null);
    }
    return;
  }

  if (sentryDsn && !sentryInitialized) {
    Sentry.init({
      dsn: sentryDsn,
      tracesSampleRate: 1.0,
      environment:
        import.meta.env.MODE === 'development' ? 'dev' : 'production',
      beforeSend: (event) => (telemetryEnabled ? event : null),
      integrations: [
        Sentry.reactRouterV6BrowserTracingIntegration({
          useEffect: React.useEffect,
          useLocation,
          useNavigationType,
          createRoutesFromChildren,
          matchRoutes,
        }),
      ],
    });
    Sentry.setTag('source', 'frontend');
    sentryInitialized = true;
  }

  if (sentryInitialized && analyticsUserId) {
    Sentry.setUser({ id: analyticsUserId });
  }

  if (posthogApiKey && posthogApiEndpoint) {
    if (!posthogInitialized) {
      posthog.init(posthogApiKey, {
        api_host: posthogApiEndpoint,
        capture_pageview: false,
        capture_pageleave: true,
        capture_performance: true,
        autocapture: false,
        opt_out_capturing_by_default: true,
      });
      posthogInitialized = true;
    }

    posthog.opt_in_capturing();
    if (analyticsUserId) {
      posthog.identify(analyticsUserId);
    }
  }
}

export { posthog };
