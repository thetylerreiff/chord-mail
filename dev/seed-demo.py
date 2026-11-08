#!/usr/bin/env python3
"""Seeds a made-up mailbox for screenshots and offscreen checks.

Usage: dev/seed-demo.py <dir>
Then:  XDG_DATA_HOME=<dir>/data XDG_CONFIG_HOME=<dir>/config CHORD_DEMO=1 chord-mail open

Every name and address here is invented; never put real mail in it.
"""

import os
import sqlite3
import subprocess
import sys
import time

root = sys.argv[1]
data = os.path.join(root, "data")
config = os.path.join(root, "config")
os.makedirs(os.path.join(config, "chord-mail"), exist_ok=True)
env = dict(os.environ, XDG_DATA_HOME=data, XDG_CONFIG_HOME=config)
binary = os.environ.get("CHORD_BIN", "target/debug/chord-mail")
# Let the app create its own schema and default config.
subprocess.run([binary, "inbox"], env=env, check=True, capture_output=True)

db = sqlite3.connect(os.path.join(data, "chord-mail", "mail.db"))
now = int(time.time() * 1000)
minute = 60_000
hour = 60 * minute
day = 24 * hour

meta = {
    "email": "me@example.com",
    "send_as_email": "me@example.com",
    "send_as_name": "Sam Example",
    "signature": "",
    "snooze_label": "Label_9",
    "account_refreshed": str(now),
    "history_id": "1",
}
for k, v in meta.items():
    db.execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", (k, v))
for lid, name, system in [
    ("Label_1", "Friends", 0),
    ("Label_2", "Receipts", 0),
    ("Label_3", "Projects/Chord", 0),
    ("Label_9", "Chord/Snoozed", 0),
    ("INBOX", "INBOX", 1),
    ("STARRED", "STARRED", 1),
]:
    db.execute("INSERT OR REPLACE INTO labels (id, name, system) VALUES (?, ?, ?)", (lid, name, system))

HTML = """<html><body style="font-family:Helvetica,Arial,sans-serif;background:#ffffff;color:#222">
<table width="100%" cellpadding="0" cellspacing="0"><tr><td style="padding:24px">
<h2 style="margin:0 0 12px">Engine notes, draft 3</h2>
<p>Here's where the Analytical Engine notes stand. I rewrote the section on the
Bernoulli numbers and added the table you asked for.</p>
<table style="border-collapse:collapse;margin:16px 0">
<tr><th style="border:1px solid #ccc;padding:6px 10px;background:#f3f3f3">Step</th><th style="border:1px solid #ccc;padding:6px 10px;background:#f3f3f3">Operation</th></tr>
<tr><td style="border:1px solid #ccc;padding:6px 10px">1</td><td style="border:1px solid #ccc;padding:6px 10px">× V2 · V3</td></tr>
<tr><td style="border:1px solid #ccc;padding:6px 10px">2</td><td style="border:1px solid #ccc;padding:6px 10px">− V4 − V1</td></tr>
</table>
<p>Can you look before Thursday? <a href="https://example.com/notes">The full draft is here.</a></p>
<img src="https://tracker.example.com/open.gif?id=8812" width="1" height="1">
<p style="color:#888">Ada</p></td></tr></table></body></html>"""


def raw(sender, to, subject, body, html=False, msgid="x", attachment=None):
    head = f"From: {sender}\r\nTo: {to}\r\nSubject: {subject}\r\nMessage-ID: <{msgid}@example.com>\r\nMIME-Version: 1.0\r\n"
    if attachment:
        name, ctype, b64 = attachment
        return (head + 'Content-Type: multipart/mixed; boundary="b"\r\n\r\n'
                f"--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
                f'--b\r\nContent-Type: {ctype}; name="{name}"\r\nContent-Disposition: attachment; filename="{name}"\r\n'
                f"Content-Transfer-Encoding: base64\r\n\r\n{b64}\r\n--b--\r\n").encode()
    ctype = "text/html" if html else "text/plain"
    return (head + f"Content-Type: {ctype}; charset=utf-8\r\n\r\n{body}").encode()


messages = [
    # id, thread, age, from, to, subject, snippet, labels, raw
    ("m1", "t1", 12 * minute, "Ada Lovelace <ada@example.com>", "me@example.com", "Engine notes, draft 3",
     "Here's where the Analytical Engine notes stand. I rewrote the section on the Bernoulli numbers",
     ["INBOX", "UNREAD", "CATEGORY_PERSONAL", "Label_3"], raw("Ada Lovelace <ada@example.com>", "me@example.com", "Engine notes, draft 3", HTML, html=True, msgid="m1")),
    ("m2", "t2", 50 * minute, "Mom <mom@example.com>", "me@example.com", "Dinner Sunday?",
     "Are you two coming Sunday? I'm making the lemon chicken. Let me know by Friday",
     ["INBOX", "UNREAD", "CATEGORY_PERSONAL", "Label_1"], raw("Mom <mom@example.com>", "me@example.com", "Dinner Sunday?",
     "Are you two coming Sunday? I'm making the lemon chicken.\n\nLet me know by Friday so I can shop.\n\nLove,\nMom", msgid="m2")),
    ("m3a", "t3", 2 * day, "Grace Hopper <grace@example.com>", "me@example.com", "Compiler talk",
     "Would you give the compiler talk at the meetup on the 19th?", ["CATEGORY_PERSONAL"],
     raw("Grace Hopper <grace@example.com>", "me@example.com", "Compiler talk", "Would you give the compiler talk at the meetup on the 19th? Thirty minutes, questions after.\n\nGrace", msgid="m3a")),
    ("m3b", "t3", 1 * day, "Sam Example <me@example.com>", "grace@example.com", "Re: Compiler talk",
     "Happy to. Can I bring the demo on the old laptop?", ["SENT"],
     raw("Sam Example <me@example.com>", "grace@example.com", "Re: Compiler talk", "Happy to. Can I bring the demo on the old laptop?\n\nOn Tue, Grace Hopper wrote:\n> Would you give the compiler talk at the meetup on the 19th?", msgid="m3b")),
    ("m3c", "t3", 3 * hour, "Grace Hopper <grace@example.com>", "me@example.com", "Re: Compiler talk",
     "Bring it! We'll have a projector and a very long extension cord.", ["INBOX", "UNREAD", "CATEGORY_PERSONAL"],
     raw("Grace Hopper <grace@example.com>", "me@example.com", "Re: Compiler talk",
         "Bring it! We'll have a projector and a very long extension cord.\n\nhttps://example.com/meetup/19\n\nOn Wed, Sam Example wrote:\n> Happy to. Can I bring the demo on the old laptop?\n>\n> > Would you give the compiler talk?", msgid="m3c")),
    ("m4", "t4", 5 * hour, "Katherine Johnson <katherine@example.com>", "me@example.com", "Trajectory numbers",
     "Attached are the corrected numbers for the second stage.", ["INBOX", "CATEGORY_PERSONAL"],
     raw("Katherine Johnson <katherine@example.com>", "me@example.com", "Trajectory numbers", "Attached are the corrected numbers for the second stage. Check row 14.\n\nK.",
         msgid="m4", attachment=("trajectory.csv", "text/csv", "c3RhZ2UsdmVsb2NpdHkKMSw3ODAwCjIsMTEyMDAK"))),
    ("m5", "t5", 1 * day, "Alan Turing <alan@example.com>", "me@example.com", "Lunch Thursday?",
     "There's a new place by the park that does excellent soup.", ["INBOX", "STARRED", "CATEGORY_PERSONAL", "Label_1"],
     raw("Alan Turing <alan@example.com>", "me@example.com", "Lunch Thursday?", "There's a new place by the park that does excellent soup. Noon?", msgid="m5")),
    ("m6", "t6", 2 * hour, "Linear <receipts@example.com>", "me@example.com", "Your receipt from Linear",
     "Receipt #2291 · $8.00 paid with Visa ending 4242", ["INBOX", "CATEGORY_UPDATES", "Label_2"],
     raw("Linear <receipts@example.com>", "me@example.com", "Your receipt from Linear", "Receipt #2291\n$8.00 paid with Visa ending 4242", msgid="m6")),
    ("m7", "t7", 4 * hour, "GitHub <notifications@example.com>", "me@example.com", "[chord] Split inbox rules (#4)",
     "Merged #4 into main. 3 files changed.", ["INBOX", "UNREAD", "CATEGORY_UPDATES"],
     raw("GitHub <notifications@example.com>", "me@example.com", "[chord] Split inbox rules (#4)", "Merged #4 into main.\n3 files changed.", msgid="m7")),
    ("m8", "t8", 6 * hour, "The Bike Shop <news@example.com>", "me@example.com", "Winter tune-ups are 20% off",
     "Get your bike ready for the snow. Book before December 1.", ["INBOX", "CATEGORY_PROMOTIONS"],
     raw("The Bike Shop <news@example.com>", "me@example.com", "Winter tune-ups are 20% off", "Get your bike ready for the snow.", msgid="m8")),
    ("m9", "t9", 3 * day, "Marie Curie <marie@example.com>", "me@example.com", "Lab keys",
     "I left the lab keys with the front desk.", ["Label_9", "CATEGORY_PERSONAL"],
     raw("Marie Curie <marie@example.com>", "me@example.com", "Lab keys", "I left the lab keys with the front desk.", msgid="m9")),
]

for mid, thread, age, sender, to, subject, snippet, labels, body in messages:
    db.execute(
        "INSERT OR REPLACE INTO messages (id, thread_id, date, sender, to_addrs, cc_addrs, subject, snippet, size) VALUES (?,?,?,?,?,?,?,?,?)",
        (mid, thread, now - age, sender, to, "", subject, snippet, len(body)),
    )
    db.execute("DELETE FROM message_labels WHERE message_id = ?", (mid,))
    for label in labels:
        db.execute("INSERT INTO message_labels (message_id, label) VALUES (?, ?)", (mid, label))
    db.execute("INSERT OR REPLACE INTO bodies (message_id, raw) VALUES (?, ?)", (mid, body))
    db.execute("INSERT OR IGNORE INTO complete_threads (thread_id) VALUES (?)", (thread,))
    db.execute("INSERT OR IGNORE INTO notified (message_id) VALUES (?)", (mid,))

# What sync learns from headers, so typed names resolve when composing.
for i, (address, name) in enumerate([
    ("ada@example.com", "Ada Lovelace"), ("grace@example.com", "Grace Hopper"),
    ("alan@example.com", "Alan Turing"), ("mom@example.com", "Mom"),
    ("katherine@example.com", "Katherine Johnson"),
]):
    db.execute("INSERT OR REPLACE INTO contacts (address, name, count, last) VALUES (?, ?, ?, ?)", (address, name, 5 - i, now))

db.execute("INSERT OR REPLACE INTO snoozes (thread_id, until, kind, since) VALUES ('t9', ?, 'snooze', ?)", (now + 2 * day, now))
db.commit()
print(f"seeded {len(messages)} messages into {data}")
