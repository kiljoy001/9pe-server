;; Simple WASM translator example in WAT format
;; This translator just uppercases file contents

(module
  ;; Import logging function from host
  (import "ninep" "log" (func $log (param i32)))

  ;; Memory for storing data
  (memory $mem 1)
  (export "memory" (memory $mem))

  ;; Read file function
  ;; Parameters: path_ptr, path_len
  ;; Returns: data_ptr (0 if not found)
  (func $read_file (param $path_ptr i32) (param $path_len i32) (result i32)
    ;; For demo, just return a fixed response at memory location 0
    ;; Store "HELLO FROM WASM" at memory location 0
    (i32.store8 (i32.const 0) (i32.const 72))  ;; H
    (i32.store8 (i32.const 1) (i32.const 69))  ;; E
    (i32.store8 (i32.const 2) (i32.const 76))  ;; L
    (i32.store8 (i32.const 3) (i32.const 76))  ;; L
    (i32.store8 (i32.const 4) (i32.const 79))  ;; O
    (i32.store8 (i32.const 5) (i32.const 32))  ;; space
    (i32.store8 (i32.const 6) (i32.const 70))  ;; F
    (i32.store8 (i32.const 7) (i32.const 82))  ;; R
    (i32.store8 (i32.const 8) (i32.const 79))  ;; O
    (i32.store8 (i32.const 9) (i32.const 77))  ;; M
    (i32.store8 (i32.const 10) (i32.const 32)) ;; space
    (i32.store8 (i32.const 11) (i32.const 87)) ;; W
    (i32.store8 (i32.const 12) (i32.const 65)) ;; A
    (i32.store8 (i32.const 13) (i32.const 83)) ;; S
    (i32.store8 (i32.const 14) (i32.const 77)) ;; M

    ;; Log that we handled a read
    (call $log (i32.const 1))

    ;; Return pointer to data (0) and length (15)
    (i32.const 0)
  )
  (export "read_file" (func $read_file))

  ;; Write file function
  ;; Parameters: path_ptr, path_len, data_ptr, data_len
  ;; Returns: 0 for success, 1 for error
  (func $write_file (param $path_ptr i32) (param $path_len i32) (param $data_ptr i32) (param $data_len i32) (result i32)
    ;; Log that we handled a write
    (call $log (i32.const 2))
    ;; Return success
    (i32.const 0)
  )
  (export "write_file" (func $write_file))

  ;; List files function
  ;; Parameters: path_ptr, path_len
  ;; Returns: list_ptr (0 if empty)
  (func $list_files (param $path_ptr i32) (param $path_len i32) (result i32)
    ;; Log that we handled a list
    (call $log (i32.const 3))
    ;; Return empty list
    (i32.const 0)
  )
  (export "list_files" (func $list_files))
)