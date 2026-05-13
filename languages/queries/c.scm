; C symbol extraction queries

; Function definitions (direct return type)
(function_definition declarator: (function_declarator declarator: (identifier) @symbol.name)) @symbol.def

; Function definitions (pointer return type)
(function_definition declarator: (pointer_declarator declarator: (function_declarator declarator: (identifier) @symbol.name))) @symbol.def

; Struct definitions
(struct_specifier name: (type_identifier) @symbol.name) @symbol.def

; Enum definitions
(enum_specifier name: (type_identifier) @symbol.name) @symbol.def

; Type definitions
(type_definition declarator: (type_identifier) @symbol.name) @symbol.def

; Macro definitions
(preproc_function_def name: (identifier) @symbol.name) @symbol.def
(preproc_def name: (identifier) @symbol.name) @symbol.def
