; Python edge extraction queries

; Function calls
(call function: (identifier) @call.name)
(call function: (attribute attribute: (identifier) @call.name))

; Imports
(import_from_statement module_name: (dotted_name) @import.path)
(import_statement name: (dotted_name) @import.path)
