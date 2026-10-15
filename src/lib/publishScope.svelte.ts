import { publishChoices, setPublishChoice } from "./ipc";
import { explicitChoice, isIncluded, relativeTo, type PublishChoice } from "./publishScope.helpers";

/** What goes out when the open notes folder is published, for the sidebar and the Add version dialog. */
class PublishScope {
  root = $state<string | null>(null);
  choices = $state<PublishChoice[]>([]);

  async load(root: string): Promise<void> {
    // A reload of the folder already loaded keeps showing its choices until the new ones arrive
    // (and keeps them if reading fails); only a different folder starts empty.
    if (this.root !== root) {
      this.root = root;
      this.choices = [];
    }
    const found = await publishChoices(root);
    if (this.root === root) this.choices = found;
  }

  isIncluded(absPath: string): boolean {
    const rel = this.root ? relativeTo(this.root, absPath) : null;
    return rel !== null && isIncluded(rel, this.choices);
  }

  explicit(absPath: string): PublishChoice | null {
    const rel = this.root ? relativeTo(this.root, absPath) : null;
    return rel === null ? null : explicitChoice(rel, this.choices);
  }

  async choose(absPath: string, choice: boolean | null): Promise<void> {
    const root = this.root;
    const rel = root ? relativeTo(root, absPath) : null;
    if (root === null || rel === null) return;
    const updated = await setPublishChoice(root, rel, choice);
    if (this.root === root) this.choices = updated;
  }
}

export const publishScope = new PublishScope();
