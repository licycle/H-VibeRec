/**
 * Text format conversion utilities
 * 文本格式转换工具
 */

/**
 * Convert plain text newlines to Markdown format
 * Converts all line breaks to paragraph separators (double newlines) for unified formatting
 *
 * @param text - The input text to convert
 * @returns Text with unified paragraph formatting
 */
export function convertTxtToMarkdown(text: string): string {
  // Step 1: Normalize line endings to \n
  let normalized = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n');

  // Step 2: Handle literal \n sequences (sometimes present in transcription files)
  // Convert literal "\n" strings to actual newlines
  normalized = normalized.replace(/\\n/g, '\n');

  // Step 3: Remove Markdown hard breaks (backslash at end of line)
  // This ensures clean conversion to paragraph style
  normalized = normalized.replace(/\\\s*\n/g, '\n');

  // Step 4: Replace any sequence of newlines with double newline (paragraph separator)
  // First collapse multiple newlines (3+) into double newlines
  let processed = normalized.replace(/\n{3,}/g, '\n\n');

  // Step 5: Convert single newlines to double newlines (paragraph separators)
  // Use a negative lookbehind/lookahead approach to avoid replacing already-double newlines
  processed = processed.replace(/([^\n])\n([^\n])/g, '$1\n\n$2');

  // Step 6: Clean up any trailing/leading whitespace on lines
  processed = processed
    .split('\n')
    .map(line => line.trim())
    .join('\n');

  // Step 7: Normalize multiple consecutive newlines back to double
  processed = processed.replace(/\n{3,}/g, '\n\n');

  return processed;
}

/**
 * Detect if a file should be converted based on its extension
 *
 * @param fileName - The name of the file
 * @returns true if the file should be converted
 */
export function shouldConvertToMarkdown(fileName: string): boolean {
  const lowerName = fileName.toLowerCase();
  return lowerName.endsWith('.txt') || lowerName.endsWith('.docx');
}

/**
 * Convert file content to Markdown based on file type
 *
 * @param content - The file content
 * @param fileName - The file name
 * @returns Converted content if needed, otherwise original content
 */
export function convertFileToMarkdown(content: string, fileName: string): string {
  // Already Markdown, no conversion needed
  if (fileName.toLowerCase().endsWith('.md')) {
    return content;
  }

  // Convert txt and other text formats
  if (shouldConvertToMarkdown(fileName)) {
    return convertTxtToMarkdown(content);
  }

  return content;
}
