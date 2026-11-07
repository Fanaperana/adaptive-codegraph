; TypeScript edge extraction queries

; ── Calls ───────────────────────────────────────────────────────────────

; f()
(call_expression function: (identifier) @call.name)
; obj.method()
(call_expression function: (member_expression object: (_) @call.receiver property: (property_identifier) @call.name))
; new Class()
(new_expression constructor: (identifier) @call.name)
(new_expression constructor: (member_expression object: (_) @call.receiver property: (property_identifier) @call.name))

; ── Imports ─────────────────────────────────────────────────────────────

; import { a } from "./x"
(import_statement (import_clause (named_imports (import_specifier name: (identifier) @import.name !alias))) source: (string) @import.path)
; import { a as b } from "./x"
(import_statement (import_clause (named_imports (import_specifier name: (identifier) @import.name alias: (identifier) @import.alias))) source: (string) @import.path)
; import D from "./x"
(import_statement (import_clause (identifier) @import.default) source: (string) @import.path)
; import * as ns from "./x"
(import_statement (import_clause (namespace_import (identifier) @import.alias)) source: (string) @import.path)

; ── Local variable types ────────────────────────────────────────────────

; const x: Type = ...
(variable_declarator name: (identifier) @binding.name type: (type_annotation (_) @binding.type))
; const x = new Type()
(variable_declarator name: (identifier) @binding.name value: (new_expression constructor: (_) @binding.type))
; const x = make() / await make()
(variable_declarator name: (identifier) @binding.name value: (call_expression function: (_) @binding.call))
(variable_declarator name: (identifier) @binding.name value: (await_expression (call_expression function: (_) @binding.call)))
; function f(x: Type)
(required_parameter pattern: (identifier) @binding.name type: (type_annotation (_) @binding.type))
(optional_parameter pattern: (identifier) @binding.name type: (type_annotation (_) @binding.type))
