import type { SelectHTMLAttributes } from "react";
import { IconChevron } from "./icons";

export default function SelectField(props: SelectHTMLAttributes<HTMLSelectElement>) {
  return <div className="native-select"><select {...props} /><IconChevron /></div>;
}
