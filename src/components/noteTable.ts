import Table from '@tiptap/extension-table'
import { Fragment, type Node as PMNode, type Schema } from '@tiptap/pm/model'

// The slice of prosemirror-markdown's serializer state this file touches
interface MarkdownState {
  write(content?: string): void
  renderInline(parent: PMNode): void
  ensureNewLine(): void
  closeBlock(node: PMNode): void
  esc: (str: string, startOfLine?: boolean) => string
}

const PIPE = /\|/g

/**
 * A GFM table cell holds one line of inline content, but an editor cell can
 * hold several paragraphs (Enter splits one), line breaks, even lists. Fold
 * whatever is in the cell into one paragraph, so the table always serializes.
 *
 * tiptap-markdown's own table serializer gives up on those shapes and — with
 * `html: false`, which Notes uses — writes the literal text `[table]` in its
 * place (and `[hardBreak]` for a break inside a cell). The note was saved
 * with its table gone the moment someone pressed Enter in a cell.
 */
function cellRun(cell: PMNode): PMNode | null {
  const schema: Schema = cell.type.schema
  const space = schema.text(' ')
  const out: PMNode[] = []

  cell.forEach((block) => {
    const before = out.length
    if (block.isTextblock) {
      block.forEach((child) => {
        if (child.type.name === 'hardBreak') {
          out.push(space)
        } else if (child.isText && child.marks.some((m) => m.type.name === 'code')) {
          // Code text is written unescaped, so protect its pipes here
          out.push(schema.text((child.text ?? '').replace(PIPE, '\\|'), child.marks))
        } else {
          out.push(child)
        }
      })
    } else {
      // A list, quote or code block inside a cell: keep the words, drop the
      // structure (textBetween puts a space between its paragraphs, where
      // textContent would glue "first" and "second" into "firstsecond")
      const text = block.textBetween(0, block.content.size, ' ', ' ').replace(/\s+/g, ' ').trim()
      if (text) out.push(schema.text(text))
    }
    if (out.length > before) out.push(space) // blocks read as one line
  })

  while (out.length && out[out.length - 1] === space) out.pop()
  if (!out.length) return null
  return schema.nodes.paragraph.create(null, Fragment.fromArray(out))
}

/**
 * Same Markdown output as tiptap-markdown's table for the shape it accepts
 * (header row, then body rows of single paragraphs), but never falls back.
 * The first row is always written as the header: a table whose header row
 * was deleted keeps its content instead of vanishing.
 */
export const NoteTable = Table.extend({
  addStorage() {
    return {
      markdown: {
        serialize(state: MarkdownState, node: PMNode) {
          // A literal "|" would end the cell early; GFM wants it escaped
          const plainEsc = state.esc
          state.esc = (str: string, startOfLine?: boolean) =>
            plainEsc.call(state, str, startOfLine).replace(PIPE, '\\|')
          try {
            node.forEach((row, _offset, rowIndex) => {
              state.write('| ')
              row.forEach((cell, _cellOffset, cellIndex) => {
                if (cellIndex) state.write(' | ')
                const run = cellRun(cell)
                if (run) state.renderInline(run)
              })
              state.write(' |')
              state.ensureNewLine()
              if (rowIndex === 0) {
                const delimiter = Array.from({ length: row.childCount }, () => '---').join(' | ')
                state.write(`| ${delimiter} |`)
                state.ensureNewLine()
              }
            })
            state.closeBlock(node)
          } finally {
            state.esc = plainEsc
          }
        },
        parse: {
          // handled by markdown-it
        },
      },
    }
  },
})
