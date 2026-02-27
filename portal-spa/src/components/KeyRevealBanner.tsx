import { useState } from 'react';

interface KeyRevealBannerProps {
  rawKey: string;
  onDismiss: () => void;
}

export function KeyRevealBanner({ rawKey, onDismiss }: KeyRevealBannerProps) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    await navigator.clipboard.writeText(rawKey);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <div className="mb-6 p-4 bg-amber-50 border border-amber-200 rounded-lg" role="alert">
      <div className="flex items-start gap-3">
        <svg
          className="w-5 h-5 text-amber-600 mt-0.5 shrink-0"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z"
          />
        </svg>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-amber-800">
            Copy your API key now. You won't be able to see it again.
          </p>
          <div className="mt-2 flex items-center gap-2">
            <code className="block px-3 py-2 bg-white border border-amber-200 rounded text-sm font-mono text-slate-900 break-all">
              {rawKey}
            </code>
            <button
              onClick={handleCopy}
              className="shrink-0 px-3 py-2 bg-amber-600 text-white text-sm font-medium rounded
                hover:bg-amber-700 focus:outline-none focus:ring-2 focus:ring-amber-500"
              aria-label="Copy API key to clipboard"
            >
              {copied ? 'Copied!' : 'Copy'}
            </button>
          </div>
        </div>
        <button
          onClick={onDismiss}
          className="shrink-0 text-amber-500 hover:text-amber-700"
          aria-label="Dismiss"
        >
          <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>
    </div>
  );
}
