# Hotel reservations

The parser accepts labelled text or HTML confirmations and reminders. It requires a hotel name, confirmation number, and RFC3339 check-in and check-out timestamps with explicit offsets. It preserves the supplied local times and offsets alongside UTC instants. Date-only or zone-less values, conflicting labels, invalid dates, and reversed stays fall back to ordinary mail. Accepted fields have confidence 1.0 and cite the current host-provided email source.

Identity is the normalized hotel name and confirmation number, independent of email threading and sender. This assumes confirmation numbers are unique within a hotel. Changed stay dates update the same reservation. A hotel's reuse of a confirmation number cannot be distinguished without another explicit booking identifier.

Semantic update keys use the hotel, confirmation number and UTC stay instants. Reminder wording, message IDs, and delivery times do not create new updates. Prior update timestamps from host context prevent late duplicate reminders from undoing newer dates. The host retains and merges source evidence and acknowledgment.

The native reservation template shows stay times with their original offsets and the confirmation number. Check-in and check-out promote the record in Coming up without synthetic inbox reminders or attention requirements. Time alone does not imply check-in, completion, or cancellation. This parser does not infer those states or call remote services.
