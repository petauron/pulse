import { addCollection, setCustomIconLoader } from '@iconify/vue'
import localIconCollections from '@/generated/icons'

/** Register only the icons referenced by Emerald so Pulse works fully offline. */
export async function setupIconify(): Promise<void> {
  for (const collection of localIconCollections) {
    addCollection(collection as unknown as Parameters<typeof addCollection>[0])
    setCustomIconLoader(() => null, collection.prefix)
  }
}
