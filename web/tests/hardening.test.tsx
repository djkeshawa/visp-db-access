import { describe, it, expect } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { SafetyPanel } from '../src/features/console/safety';
import { analyzeDemo } from '../src/api/mock-guard';
import { defaultPolicy } from '../src/api/fixtures';
import { MemoryRouter } from 'react-router-dom';
import { HistoryTable } from '../src/features/history/history';
import { ApprovalBadge } from '../src/features/approvals/approvals';
import { fixtures } from '../src/api/fixtures';
import { ApiError } from '../src/api/errors';
import { loginMessage, message } from '../src/lib/utils';

describe('hardened safety and errors', () => {
  it('shows the sentinel note alongside verbatim rewritten SQL only when the cap includes an extra row', () => {
    const policy = defaultPolicy('development');
    const analysis = analyzeDemo('SELECT * FROM users', policy);
    const view = (sql: string) =>
      renderToStaticMarkup(
        <SafetyPanel
          analysis={{ ...analysis, rewritten_sql: sql }}
          maxRows={policy.max_rows}
        />,
      );
    expect(view('SELECT * FROM users LIMIT 5001')).toContain(
      '(+1 row to detect truncation)',
    );
    expect(view('SELECT * FROM users LIMIT 17')).not.toContain('(+1 row');
    expect(view("SELECT 'LIMIT 5001' FROM users LIMIT 17")).not.toContain(
      '(+1 row',
    );
    expect(view('SELECT * FROM users LIMIT 5001 OFFSET 5')).toContain(
      '(+1 row',
    );
    expect(view('SELECT * FROM users FETCH FIRST 5001 ROWS ONLY')).toContain(
      '(+1 row',
    );
    expect(view('SELECT * FROM users LIMIT 5001')).toContain('LIMIT 5001');
  });
  it('shows masking serialization issues with actionable guidance', () => {
    const analysis = analyzeDemo(
      'SELECT * FROM users',
      defaultPolicy('development'),
    );
    analysis.verdict = 'deny';
    analysis.issues = [
      {
        severity: 'block',
        code: 'masked_data_serialization',
        message:
          'Row serialization is prohibited when the query touches masked data',
      },
    ];
    const html = renderToStaticMarkup(<SafetyPanel analysis={analysis} />);
    expect(html).toContain('Select individual columns');
  });
  it('preserves validation errors, adds retry guidance and explains login lockout without revealing account existence', () => {
    expect(
      message(
        new ApiError(
          'validation',
          'Text contains prohibited control characters',
          400,
        ),
      ),
    ).toBe('Text contains prohibited control characters');
    for (const code of ['busy', 'rate_limited'])
      expect(message(new ApiError(code, 'Try again later', 429))).toContain(
        'Wait a moment',
      );
    expect(
      loginMessage(
        new ApiError(
          'unauthenticated',
          'Authentication required or invalid credentials',
          401,
        ),
      ),
    ).toContain('wait 15 minutes');
  });
});

describe('hardened status badges', () => {
  it('keeps execution status separate from the guard verdict', () => {
    const entry = fixtures().history[0]!;
    const html = renderToStaticMarkup(
      <MemoryRouter>
        <HistoryTable
          items={[
            { ...entry, id: 'running', status: 'running' },
            { ...entry, id: 'unknown', status: 'unknown' },
          ]}
        />
      </MemoryRouter>,
    );
    expect(html).toContain('Running');
    expect(html).toContain('data-variant="verdict"');
    expect(html).toContain('Unknown');
  });
  it('renders executing approvals with consistent status variants', () => {
    expect(
      renderToStaticMarkup(<ApprovalBadge status="executing" />),
    ).toContain('badge info');
    expect(renderToStaticMarkup(<ApprovalBadge status="failed" />)).toContain(
      'badge danger',
    );
  });
});
