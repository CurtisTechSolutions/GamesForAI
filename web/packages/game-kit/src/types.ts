import type { SceneFrame } from "./scene";
export interface BoardProps extends SceneFrame {
  gameId: string;
  label: string;
  onAction: (action: string) => void;
}
