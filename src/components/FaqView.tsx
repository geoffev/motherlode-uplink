const FAQ: { q: string; a: string }[] = [
  {
    q: 'Is this safe with Easy Anti-Cheat?',
    a: 'The app never touches the Star Citizen process. It does not inject code, hook the game or read its memory. It only looks at the screen through the operating system’s screen-capture APIs, the same ones streaming and recording software uses.',
  },
  {
    q: 'What does it capture?',
    a: 'Only the Star Citizen window (the StarCitizen.exe process), never the rest of your screen. If the game isn’t running or is minimised, nothing is recorded, and the app says so. While it runs, a frame is taken once or twice a second and checked on a small copy for change. When a commodity terminal holds still on screen, one full frame is read with OCR on this computer. Nothing is uploaded except the prices you confirm.',
  },
  {
    q: 'Why do I have to confirm each reading?',
    a: 'Motherlode trusts reports as they arrive: there is no voting or plausibility check on the server. Your confirmation is the only check, so look at the crop next to each value, and fix anything that was misread. Fields that were hard to read have a gold underline.',
  },
  {
    q: 'Which terminal does a report go to?',
    a: 'The app looks for the station or city name on screen. If it can’t find one, it suggests the terminal you last confirmed. You can always pick another from the list before confirming; the choice applies to every reading from that screen.',
  },
  {
    q: 'What happens if I go offline?',
    a: 'Confirmed reports wait in the queue and go out when the connection is back. The Sent tab shows each report as queued, sent or failed; failed ones can be retried.',
  },
  {
    q: 'Where are my API key and screenshots kept?',
    a: 'The key is in your system keychain, never in a plain file. Crops of what you confirmed are kept on this computer for the period set in Settings, to help train a better OCR model; they are not uploaded.',
  },
  {
    q: 'Linux: the window is blank',
    a: 'The app turns off WebKitGTK’s DMA-BUF renderer by default, which fixes blank windows on most Nvidia + Wayland setups. If it still happens, start the app with WEBKIT_DISABLE_COMPOSITING_MODE=1.',
  },
]

export function FaqView() {
  return (
    <section className="panel faq">
      <div className="panel-head"><span className="t-label">FAQ</span></div>
      <dl className="faq-list">
        {FAQ.map(f => (
          <div key={f.q} className="faq-item">
            <dt className="t-h3">{f.q}</dt>
            <dd className="t-body">{f.a}</dd>
          </div>
        ))}
      </dl>
    </section>
  )
}
