import { ChevronLeft, ChevronRight } from "lucide-react";

interface PaginationProps {
  offset: number;
  limit: number;
  total: number;
  onPageChange: (newOffset: number) => void;
}

export function Pagination({ offset, limit, total, onPageChange }: PaginationProps) {
  if (total <= limit) return null;

  const currentPage = Math.floor(offset / limit) + 1;
  const totalPages = Math.ceil(total / limit);
  const hasPrev = offset > 0;
  const hasNext = offset + limit < total;

  function goTo(page: number) {
    onPageChange((page - 1) * limit);
  }

  // Build page numbers: show at most 5 pages centered on current
  const pages: number[] = [];
  let start = Math.max(1, currentPage - 2);
  const end = Math.min(totalPages, start + 4);
  start = Math.max(1, end - 4);
  for (let i = start; i <= end; i++) {
    pages.push(i);
  }

  return (
    <div className="flex items-center justify-between px-6 py-3 border-t border-slate-200 bg-white">
      <p className="text-sm text-slate-500">
        Showing <span className="font-medium text-slate-700">{offset + 1}</span>
        {" - "}
        <span className="font-medium text-slate-700">{Math.min(offset + limit, total)}</span>
        {" of "}
        <span className="font-medium text-slate-700">{total}</span>
      </p>
      <div className="flex items-center gap-1">
        <button
          onClick={() => goTo(currentPage - 1)}
          disabled={!hasPrev}
          className="p-1.5 rounded-lg text-slate-400 hover:text-slate-600 hover:bg-slate-100
            disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
          aria-label="Previous page"
        >
          <ChevronLeft className="w-4 h-4" />
        </button>
        {pages[0] > 1 && (
          <>
            <button
              onClick={() => goTo(1)}
              className="min-w-[32px] h-8 px-2 rounded-lg text-sm text-slate-600 hover:bg-slate-100 transition-colors"
            >
              1
            </button>
            {pages[0] > 2 && (
              <span className="px-1 text-slate-400 text-sm">...</span>
            )}
          </>
        )}
        {pages.map((p) => (
          <button
            key={p}
            onClick={() => goTo(p)}
            className={`min-w-[32px] h-8 px-2 rounded-lg text-sm font-medium transition-colors ${
              p === currentPage
                ? "bg-cyan-400 text-[#0A0F1C]"
                : "text-slate-600 hover:bg-slate-100"
            }`}
          >
            {p}
          </button>
        ))}
        {pages[pages.length - 1] < totalPages && (
          <>
            {pages[pages.length - 1] < totalPages - 1 && (
              <span className="px-1 text-slate-400 text-sm">...</span>
            )}
            <button
              onClick={() => goTo(totalPages)}
              className="min-w-[32px] h-8 px-2 rounded-lg text-sm text-slate-600 hover:bg-slate-100 transition-colors"
            >
              {totalPages}
            </button>
          </>
        )}
        <button
          onClick={() => goTo(currentPage + 1)}
          disabled={!hasNext}
          className="p-1.5 rounded-lg text-slate-400 hover:text-slate-600 hover:bg-slate-100
            disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
          aria-label="Next page"
        >
          <ChevronRight className="w-4 h-4" />
        </button>
      </div>
    </div>
  );
}
