; Go edge extraction queries

; Function calls
(call_expression function: (identifier) @call.name)
(call_expression function: (selector_expression field: (field_identifier) @call.name))

; Imports
(import_spec path: (interpreted_string_literal) @import.path)
