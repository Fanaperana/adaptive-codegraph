; TypeScript edge extraction queries

; Function calls
(call_expression function: (identifier) @call.name)
(call_expression function: (member_expression property: (property_identifier) @call.name))

; Imports
(import_statement source: (string) @import.path)
