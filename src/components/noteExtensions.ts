import type { Extensions } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import CodeBlock from '@tiptap/extension-code-block'
import Placeholder from '@tiptap/extension-placeholder'
import TaskList from '@tiptap/extension-task-list'
import TaskItem from '@tiptap/extension-task-item'
import Table from '@tiptap/extension-table'
import TableRow from '@tiptap/extension-table-row'
import TableHeader from '@tiptap/extension-table-header'
import TableCell from '@tiptap/extension-table-cell'
import { VueNodeViewRenderer } from '@tiptap/vue-3'
import { Markdown } from 'tiptap-markdown'
import { Hashtag } from './hashtagExtension'
import { NoteImages } from './noteImages'
import { TabKeys } from './tabKeysExtension'
import CodeBlockView from './CodeBlockView.vue'

// Code blocks get Copy/Run actions through a Vue node view; same node
// name, so markdown storage and the other extensions see no difference
const RunnableCodeBlock = CodeBlock.extend({
  addNodeView() {
    return VueNodeViewRenderer(CodeBlockView)
  },
})

/** The one editor setup shared by Notes and Quick Note, so markdown
 *  shortcuts (`- `, `1. `, `[] `, `# `, `*x*`, …) behave identically. */
export function noteExtensions(placeholder: string): Extensions {
  return [
    StarterKit.configure({ heading: { levels: [1, 2, 3] }, codeBlock: false }),
    RunnableCodeBlock,
    Placeholder.configure({ placeholder }),
    TaskList,
    TaskItem.configure({ nested: true }),
    Table.configure({ resizable: false }),
    TableRow,
    TableHeader,
    TableCell,
    Markdown.configure({ html: false, breaks: true, transformPastedText: true }),
    Hashtag,
    NoteImages,
    // Last on purpose: its Tab handling must run after the list/task
    // indent shortcuts so it only sees keys they didn't consume
    TabKeys,
  ]
}
