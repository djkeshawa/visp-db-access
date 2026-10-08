import { version } from '../../../package.json';
export function AboutConsole() {
  return (
    <section className="settings-section">
      <h2>About</h2>
      <dl className="review-policy">
        <dt>Application</dt>
        <dd>visp · db access</dd>
        <dt>Console version</dt>
        <dd>{version}</dd>
        <dt>API</dt>
        <dd>v1</dd>
      </dl>
      <p>
        <a href="/docs/api.md" target="_blank" rel="noreferrer">
          API documentation
        </a>{' '}
        ·{' '}
        <a href="/docs/architecture.md" target="_blank" rel="noreferrer">
          Architecture
        </a>
      </p>
    </section>
  );
}
