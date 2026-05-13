; Rust edge extraction queries

; Function calls
(call_expression function: (identifier) @call.name)
(call_expression function: (field_expression field: (field_identifier) @call.name))
(call_expression function: (scoped_identifier name: (identifier) @call.name))

; Use imports
(use_declaration argument: (scoped_identifier) @import.path)
(use_declaration argument: (identifier) @import.path)
