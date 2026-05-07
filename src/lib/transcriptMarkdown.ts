export function transcriptToMarkdown(title: string, text: string): string {
  const body = text
    .replace(/\r\n/g, '\n')
    .replace(/\r/g, '\n')
    .split('\n')
    .map(line => line.trim())
    .filter(Boolean)
    .join('\n\n');
  return `# ${title}\n\n${body}`;
}
