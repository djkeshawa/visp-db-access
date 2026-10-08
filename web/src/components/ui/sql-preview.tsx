import { sqlParts } from '../../lib/activity';
/** React text nodes keep SQL previews inert, including strings that resemble HTML. */
export function SqlPreview({ sql }: { sql: string }) {
  return (
    <code>
      {sqlParts(sql).map((part, index) => (
        <span key={index} className={`sql-${part.kind}`}>
          {part.text}
        </span>
      ))}
    </code>
  );
}
