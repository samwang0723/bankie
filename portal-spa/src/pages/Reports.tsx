import { useState } from 'react';
import { FileText, Download, Calendar, AlertCircle } from 'lucide-react';

const CURRENCIES = [
  { label: 'All Currencies', value: 'all' },
  { label: 'USD', value: 'USD' },
  { label: 'TWD', value: 'TWD' },
  { label: 'BTC', value: 'BTC' },
  { label: 'ETH', value: 'ETH' },
  { label: 'USDT', value: 'USDT' },
];

function downloadReport(params: { start_date: string; end_date: string; currency?: string }) {
  const searchParams = new URLSearchParams();
  searchParams.set('start_date', params.start_date);
  searchParams.set('end_date', params.end_date);
  if (params.currency && params.currency !== 'all') {
    searchParams.set('currency', params.currency);
  }
  window.open(`/api/portal/v1/data/reports/settlement?${searchParams}`, '_blank');
}

interface GeneratedReport {
  name: string;
  startDate: string;
  endDate: string;
  currency: string;
  generatedAt: string;
}

export function Reports() {
  const [startDate, setStartDate] = useState('');
  const [endDate, setEndDate] = useState('');
  const [currency, setCurrency] = useState('all');
  const [error, setError] = useState<string | null>(null);
  const [recentReports, setRecentReports] = useState<GeneratedReport[]>([]);

  function handleGenerate() {
    setError(null);

    if (!startDate || !endDate) {
      setError('Please select both start and end dates.');
      return;
    }

    const start = new Date(startDate);
    const end = new Date(endDate);

    if (start > end) {
      setError('Start date must be before end date.');
      return;
    }

    const daysDiff = Math.ceil((end.getTime() - start.getTime()) / (1000 * 60 * 60 * 24));
    if (daysDiff > 90) {
      setError('Date range cannot exceed 90 days.');
      return;
    }

    // Track in recent reports
    const report: GeneratedReport = {
      name: `Settlement_${startDate}_to_${endDate}`,
      startDate,
      endDate,
      currency: currency === 'all' ? 'All' : currency,
      generatedAt: new Date().toISOString(),
    };
    setRecentReports((prev) => [report, ...prev].slice(0, 10));

    downloadReport({ start_date: startDate, end_date: endDate, currency });
  }

  function formatDate(dateStr: string): string {
    return new Date(dateStr).toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
    });
  }

  return (
    <div>
      {/* Page header */}
      <div className="mb-8">
        <div className="flex items-center gap-3 mb-1">
          <FileText className="w-6 h-6 text-cyan-400" />
          <h1 className="text-2xl font-bold text-slate-900">Reports</h1>
        </div>
        <p className="mt-1 text-sm text-slate-500">
          Generate and download settlement reports.
        </p>
      </div>

      {/* Generate Settlement Report */}
      <div className="bg-white rounded-xl border border-slate-200 p-6 mb-6">
        <h2 className="text-base font-semibold text-slate-900 mb-2">
          Generate Settlement Report
        </h2>
        <p className="text-sm text-slate-500 mb-6">
          Select a date range and optional filters to generate a CSV settlement report with
          double-entry journal data and running balances.
        </p>

        {error && (
          <div className="flex items-center gap-2 mb-4 p-3 bg-red-50 border border-red-200 rounded-lg">
            <AlertCircle className="w-4 h-4 text-red-500 shrink-0" />
            <p className="text-sm text-red-700">{error}</p>
          </div>
        )}

        <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1.5">
              Start Date
            </label>
            <div className="relative">
              <Calendar className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400 pointer-events-none" />
              <input
                type="date"
                value={startDate}
                onChange={(e) => setStartDate(e.target.value)}
                className="w-full h-10 pl-10 pr-3 bg-white border border-slate-200 rounded-lg text-sm text-slate-900
                  focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent"
              />
            </div>
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1.5">
              End Date
            </label>
            <div className="relative">
              <Calendar className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400 pointer-events-none" />
              <input
                type="date"
                value={endDate}
                onChange={(e) => setEndDate(e.target.value)}
                className="w-full h-10 pl-10 pr-3 bg-white border border-slate-200 rounded-lg text-sm text-slate-900
                  focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent"
              />
            </div>
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1.5">
              Currency
            </label>
            <select
              value={currency}
              onChange={(e) => setCurrency(e.target.value)}
              className="w-full h-10 px-3 bg-white border border-slate-200 rounded-lg text-sm text-slate-900
                focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent"
            >
              {CURRENCIES.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
          </div>
        </div>

        <button
          onClick={handleGenerate}
          className="flex items-center gap-2 h-10 px-6 bg-cyan-400 text-[#0A0F1C] text-sm font-semibold rounded-lg
            hover:bg-cyan-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:ring-offset-2"
        >
          <Download className="w-4 h-4" />
          Generate Report
        </button>
      </div>

      {/* Recent Reports */}
      <div className="bg-white rounded-xl border border-slate-200 p-6">
        <h2 className="text-base font-semibold text-slate-900 mb-4">Recent Reports</h2>

        {recentReports.length === 0 ? (
          <div className="py-8 text-center">
            <FileText className="w-10 h-10 text-slate-300 mx-auto mb-3" />
            <p className="text-slate-500 mb-1">No reports generated yet</p>
            <p className="text-sm text-slate-400">
              Use the form above to generate your first settlement report.
            </p>
          </div>
        ) : (
          <div className="overflow-hidden">
            <table className="w-full" aria-label="Recent reports">
              <thead>
                <tr className="border-b border-slate-200">
                  <th className="text-left px-4 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                    Report
                  </th>
                  <th className="text-left px-4 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                    Date Range
                  </th>
                  <th className="text-left px-4 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                    Currency
                  </th>
                  <th className="text-left px-4 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                    Status
                  </th>
                  <th className="text-right px-4 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono w-[100px]">
                    Action
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-200">
                {recentReports.map((report, i) => (
                  <tr key={i} className="hover:bg-slate-50">
                    <td className="px-4 py-3 text-sm font-medium text-slate-900">
                      {report.name}
                    </td>
                    <td className="px-4 py-3 text-sm text-slate-600">
                      {formatDate(report.startDate)} - {formatDate(report.endDate)}
                    </td>
                    <td className="px-4 py-3 text-sm font-medium text-slate-900">
                      {report.currency}
                    </td>
                    <td className="px-4 py-3">
                      <span className="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-green-50 text-green-700">
                        Ready
                      </span>
                    </td>
                    <td className="px-4 py-3 text-right">
                      <button
                        onClick={() =>
                          downloadReport({
                            start_date: report.startDate,
                            end_date: report.endDate,
                            currency: report.currency === 'All' ? 'all' : report.currency,
                          })
                        }
                        className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-cyan-700
                          bg-cyan-50 rounded-md hover:bg-cyan-100 transition-colors"
                      >
                        <Download className="w-3.5 h-3.5" />
                        CSV
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
