import type { ReactNode } from "react";
import { marked, type Token, type Tokens } from "marked";

type MarkdownContentProps = {
  content: string;
  className?: string;
};

const TOKEN_CACHE_MAX = 500;
const tokenCache = new Map<string, Token[]>();
const markdownSyntaxPattern = /[#*`|[>\-_~]|\n\n|^\d+\. |\n\d+\. |\[[ xX]\]|\[[^\]]+\]\([^)]+\)/;

export function MarkdownContent({ content, className }: MarkdownContentProps) {
  const tokens = tokensForMarkdown(content.replace(/\r\n/g, "\n"));
  const classes = ["markdown-content", className].filter(Boolean).join(" ");
  return (
    <div className={classes}>
      {tokens.map((token, index) => renderBlockToken(token, `md-${index}`))}
    </div>
  );
}

function tokensForMarkdown(content: string): Token[] {
  if (!hasMarkdownSyntax(content)) {
    return [
      {
        type: "paragraph",
        raw: content,
        text: content,
        tokens: [{ type: "text", raw: content, text: content }],
      } as Token,
    ];
  }
  const key = hashContent(content);
  const cached = tokenCache.get(key);
  if (cached) {
    tokenCache.delete(key);
    tokenCache.set(key, cached);
    return cached;
  }
  const tokens = marked.lexer(content, { gfm: true }) as Token[];
  if (tokenCache.size >= TOKEN_CACHE_MAX) {
    const oldest = tokenCache.keys().next().value;
    if (oldest !== undefined) {
      tokenCache.delete(oldest);
    }
  }
  tokenCache.set(key, tokens);
  return tokens;
}

function hasMarkdownSyntax(content: string): boolean {
  const sample = content.length > 500 ? content.slice(0, 500) : content;
  return markdownSyntaxPattern.test(sample);
}

function hashContent(content: string): string {
  let hash = 2166136261;
  for (let index = 0; index < content.length; index++) {
    hash ^= content.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return `${content.length}:${(hash >>> 0).toString(36)}`;
}

function renderBlockTokens(tokens: Token[], keyPrefix: string): ReactNode[] {
  return tokens.map((token, index) => renderBlockToken(token, `${keyPrefix}-${index}`));
}

function renderInlineTokens(tokens: Token[] | undefined, keyPrefix: string): ReactNode[] {
  return (tokens ?? []).map((token, index) => renderInlineToken(token, `${keyPrefix}-${index}`));
}

function renderBlockToken(token: Token, key: string): ReactNode {
  switch (token.type) {
    case "space":
    case "def":
      return null;
    case "paragraph": {
      const paragraph = token as Tokens.Paragraph;
      return <p key={key}>{renderInlineTokens(paragraph.tokens, key)}</p>;
    }
    case "heading": {
      const heading = token as Tokens.Heading;
      const children = renderInlineTokens(heading.tokens, key);
      if (heading.depth === 1) return <h2 key={key}>{children}</h2>;
      if (heading.depth === 2) return <h3 key={key}>{children}</h3>;
      return <h4 key={key}>{children}</h4>;
    }
    case "blockquote": {
      const blockquote = token as Tokens.Blockquote;
      return <blockquote key={key}>{renderBlockTokens(blockquote.tokens, key)}</blockquote>;
    }
    case "code": {
      const code = token as Tokens.Code;
      return (
        <pre key={key}>
          <code className={code.lang ? `language-${code.lang}` : undefined}>{code.text}</code>
        </pre>
      );
    }
    case "hr":
      return <hr key={key} />;
    case "list":
      return renderList(token as Tokens.List, key);
    case "table":
      return renderTable(token as Tokens.Table, key);
    case "html": {
      const html = token as Tokens.HTML;
      return <p key={key}>{html.raw || html.text}</p>;
    }
    case "text": {
      const text = token as Tokens.Text;
      return <p key={key}>{text.tokens ? renderInlineTokens(text.tokens, key) : text.text}</p>;
    }
    default:
      return <p key={key}>{token.raw}</p>;
  }
}

function renderInlineToken(token: Token, key: string): ReactNode {
  switch (token.type) {
    case "text": {
      const text = token as Tokens.Text;
      return text.tokens ? renderInlineTokens(text.tokens, key) : text.text;
    }
    case "escape":
      return (token as Tokens.Escape).text;
    case "strong":
      return <strong key={key}>{renderInlineTokens((token as Tokens.Strong).tokens, key)}</strong>;
    case "em":
      return <em key={key}>{renderInlineTokens((token as Tokens.Em).tokens, key)}</em>;
    case "del":
      return <del key={key}>{renderInlineTokens((token as Tokens.Del).tokens, key)}</del>;
    case "codespan":
      return <code key={key}>{(token as Tokens.Codespan).text}</code>;
    case "br":
      return <br key={key} />;
    case "link":
      return renderLink(token as Tokens.Link, key);
    case "image": {
      const image = token as Tokens.Image;
      return image.text || image.href;
    }
    case "html": {
      const html = token as Tokens.HTML;
      return html.raw || html.text;
    }
    case "checkbox": {
      const checkbox = token as Tokens.Checkbox;
      return <input key={key} type="checkbox" checked={checkbox.checked} disabled tabIndex={-1} />;
    }
    default:
      return token.raw;
  }
}

function renderList(token: Tokens.List, key: string): ReactNode {
  const ListTag = token.ordered ? "ol" : "ul";
  const start = typeof token.start === "number" ? token.start : undefined;
  return (
    <ListTag key={key} start={start}>
      {token.items.map((item, index) => (
        <li key={`${key}-${index}`} className={item.task ? "markdown-task-item" : undefined}>
          {item.task ? (
            <input type="checkbox" checked={item.checked ?? false} disabled tabIndex={-1} />
          ) : null}
          <div className="markdown-list-item-content">{renderBlockTokens(item.tokens, `${key}-${index}`)}</div>
        </li>
      ))}
    </ListTag>
  );
}

function renderTable(token: Tokens.Table, key: string): ReactNode {
  return (
    <div key={key} className="markdown-table-wrap">
      <table>
        <thead>
          <tr>
            {token.header.map((cell, index) => (
              <th key={`${key}-h-${index}`} className={tableCellAlignClass(cell.align)}>
                {renderInlineTokens(cell.tokens, `${key}-h-${index}`)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {token.rows.map((row, rowIndex) => (
            <tr key={`${key}-r-${rowIndex}`}>
              {row.map((cell, cellIndex) => (
                <td key={`${key}-r-${rowIndex}-${cellIndex}`} className={tableCellAlignClass(cell.align)}>
                  {renderInlineTokens(cell.tokens, `${key}-r-${rowIndex}-${cellIndex}`)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function renderLink(token: Tokens.Link, key: string): ReactNode {
  const href = safeLinkHref(token.href);
  const children = renderInlineTokens(token.tokens, key);
  if (!href) {
    return <span key={key}>{children}</span>;
  }
  return (
    <a
      key={key}
      href={href}
      target={href.startsWith("#") ? undefined : "_blank"}
      rel={href.startsWith("#") ? undefined : "noreferrer"}
    >
      {children}
    </a>
  );
}

function safeLinkHref(href: string): string | null {
  const value = href.trim();
  if (
    value.startsWith("http://") ||
    value.startsWith("https://") ||
    value.startsWith("mailto:") ||
    value.startsWith("#")
  ) {
    return value;
  }
  return null;
}

function tableCellAlignClass(align: Tokens.TableCell["align"]): string | undefined {
  switch (align) {
    case "center":
      return "markdown-align-center";
    case "right":
      return "markdown-align-right";
    case "left":
    case null:
      return undefined;
  }
}
