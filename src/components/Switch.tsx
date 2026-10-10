interface Props { checked: boolean; onChange: () => void; label: string; disabled?: boolean; }

export default function Switch({ checked, onChange, label, disabled }: Props) {
  return <button type="button" className="switch" role="switch" aria-checked={checked} aria-label={label} onClick={onChange} disabled={disabled} />;
}
