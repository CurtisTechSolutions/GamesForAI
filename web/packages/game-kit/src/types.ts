import type { SceneFrame } from "./scene";
export interface BoardProps extends SceneFrame {
  gameId: string;
  readOnly?: boolean;
  label: string;
  onAction: (action: string) => void;
}
