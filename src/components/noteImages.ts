import { Extension, type Editor } from '@tiptap/core'
import Image from '@tiptap/extension-image'
import { Plugin } from '@tiptap/pm/state'
import { convertFileSrc } from '@tauri-apps/api/core'
import { api } from '../services/api'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import { IMAGE_EXTS, blobToBase64, imageFiles } from '../utils/imageData'

// Parentheses would end the markdown image URL early
function markdownSafe(url: string): string {
  return url.replace(/\(/g, '%28').replace(/\)/g, '%29')
}

async function insertImages(editor: Editor, files: File[]) {
  for (const file of files) {
    try {
      const ext = IMAGE_EXTS[file.type]
      const path = await api.saveNoteImage(await blobToBase64(file), ext)
      // The user may have opened another note while the image was being saved.
      // Each note has its own editor, so this one is gone: the picture stays on
      // disk but has nowhere to go (and a destroyed editor throws, which used to
      // surface as a misleading "Could not add image" toast).
      if (editor.isDestroyed) return
      editor
        .chain()
        .focus()
        .setImage({ src: markdownSafe(convertFileSrc(path)), alt: '' })
        .run()
    } catch (e) {
      useUiStore().toast('error', `Could not add image: ${errorText(e)}`)
    }
  }
}

/** Pasting or dropping an image stores it in app data and inlines it at
 *  the caret, the way Apple Notes does. Text paste is left untouched. */
export const NoteImages = Extension.create({
  name: 'noteImages',

  addExtensions() {
    return [Image.configure({ inline: false, allowBase64: false })]
  },

  addProseMirrorPlugins() {
    const editor = this.editor
    return [
      new Plugin({
        props: {
          handlePaste(_view, event) {
            const files = imageFiles(event.clipboardData?.items)
            if (files.length === 0) return false
            event.preventDefault()
            void insertImages(editor, files)
            return true
          },
          handleDrop(view, event, _slice, moved) {
            if (moved) return false
            const files = imageFiles(event.dataTransfer?.items)
            if (files.length === 0) return false
            event.preventDefault()
            const pos = view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos
            if (pos !== undefined) editor.chain().setTextSelection(pos).run()
            void insertImages(editor, files)
            return true
          },
        },
      }),
    ]
  },
})
