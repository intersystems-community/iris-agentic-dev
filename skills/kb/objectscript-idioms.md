# ObjectScript Idioms Reference

Canonical patterns for common ObjectScript tasks. Use alongside `docs_introspect` and
`objectscript-review` skill. These are patterns the AI should prefer — deviations from
these are often bugs.

---

## 1. Method Return Patterns

### %Status-returning method (most common)
```objectscript
ClassMethod DoWork(pInput As %String) As %Status
{
    Set sc = $$$OK
    Try {
        If pInput = "" $$$ThrowStatus($$$ERROR($$$GeneralError, "Input required"))
        // ... work ...
        $$$ThrowOnError(..HelperMethod(pInput))
    } Catch ex {
        Set sc = ex.AsStatus()
    }
    Return sc
}
```

### Value-returning method (no %Status)
```objectscript
ClassMethod GetLabel(pCode As %String) As %String
{
    Return $Case(pCode, "A":"Active", "I":"Inactive", :"Unknown")
}
```

### Method that both returns a value and can fail — use ByRef output param
```objectscript
ClassMethod Lookup(pKey As %String, Output pValue As %String) As %Status
{
    Set pValue = ""
    Set pValue = $Get(^MyData(pKey))
    If pValue = "" Return $$$ERROR($$$GeneralError, "Key not found: " _ pKey)
    Return $$$OK
}
```

---

## 2. Global Access Patterns

### Safe read with default
```objectscript
Set value = $Get(^MyGlobal(key), "default")
```

### Check-then-read
```objectscript
If $Data(^MyGlobal(key)) {
    Set value = ^MyGlobal(key)
}
```

### Iterate all subscripts
```objectscript
Set key = ""
For {
    Set key = $Order(^MyGlobal(key))
    Quit:key=""
    Set value = ^MyGlobal(key)
    // process value
}
```

### Kill only a subscript, not the whole global
```objectscript
Kill ^MyGlobal(specificKey)   // kills only this node
Kill ^MyGlobal               // kills the entire global — usually wrong
```

---

## 3. String Manipulation

### Piece (CSV-style split)
```objectscript
Set field1 = $Piece(record, ",", 1)
Set field2 = $Piece(record, ",", 2)
Set lastField = $Piece(record, ",", *)  // last piece
```

### Replace a piece
```objectscript
Set $Piece(record, ",", 2) = "new value"
```

### Extract characters
```objectscript
Set first = $Extract(str, 1)       // first char (1-based, not 0-based!)
Set sub = $Extract(str, 3, 7)     // chars 3 through 7
Set last = $Extract(str, *)        // last character
```

### Check if string contains substring
```objectscript
If str [ "substring" { ... }       // contains operator
If str '[ "substring" { ... }      // does not contain
```

### Concatenate
```objectscript
Set result = part1 _ " " _ part2  // use _ not +
```

---

## 4. List Operations

### Build a list
```objectscript
Set list = $ListBuild("a", "b", "c")
```

### Append to a list
```objectscript
Set list = list _ $ListBuild(newItem)
```

### Iterate a list (correct pattern)
```objectscript
Set ptr = 0
While $ListNext(list, ptr, item) {
    // process item
}
```

### Get item by position
```objectscript
Set item = $List(list, 2)     // 2nd item (1-based)
Set len = $ListLength(list)
```

---

## 5. Object Patterns

### Open a persistent object
```objectscript
Set obj = ##class(MyPackage.MyClass).%OpenId(id, , .sc)
If $$$ISERR(sc) { ... }
If '$IsObject(obj) { /* not found */ }
```

### Save a persistent object
```objectscript
Set sc = obj.%Save()
$$$ThrowOnError(sc)
```

### SQL query via %SQL.Statement
```objectscript
Set stmt = ##class(%SQL.Statement).%New()
Set sc = stmt.%Prepare("SELECT Name, Age FROM MyPackage.Person WHERE Age > ?")
$$$ThrowOnError(sc)
Set result = stmt.%Execute(18)
While result.%Next() {
    Set name = result.%Get("Name")
    Set age = result.%Get("Age")
}
```

---

## 6. Date and Time

### Current timestamp (IRIS internal format)
```objectscript
Set now = $ZDateTimeH($ZDateTime($H, 3), 3)  // UTC
Set nowLocal = $H                              // local $Horolog
```

### Convert $Horolog to display string
```objectscript
Set display = $ZDateTime($H, 3)   // "YYYY-MM-DD HH:MM:SS"
```

### %TimeStamp format (always use space, never T)
```objectscript
Set ts = "2025-03-15 14:30:00"   // correct
// NOT: "2025-03-15T14:30:00"    // wrong — IRIS does not accept T separator
```

---

## 7. Error Handling in Loops

Errors inside loops need careful handling — do not break the loop unless the error is fatal:

```objectscript
Set sc = $$$OK
Set key = ""
For {
    Set key = $Order(^MyData(key))
    Quit:key=""
    
    Set itemSC = ..ProcessItem(key)
    If $$$ISERR(itemSC) {
        // Log and continue, or merge into sc and continue
        Set sc = $System.Status.AppendStatus(sc, itemSC)
    }
}
```

---

## 8. Namespace Switching

```objectscript
Set savedNS = $Namespace
New $Namespace
Set $Namespace = "HSCUSTOM"
Try {
    // work in HSCUSTOM
} Catch ex {
    Set $Namespace = savedNS
    Throw ex
}
Set $Namespace = savedNS
```

---

## Anti-patterns to Avoid

| Anti-pattern | Why wrong | Correct |
|---|---|---|
| `If sc = 0` | Fragile: 0 = $$$OK but semantics can shift | `If $$$ISOK(sc)` |
| `Throw sc` | Throws a %Status, not an exception | `$$$ThrowStatus(sc)` |
| `Return` inside Try/Catch | Skips Catch cleanup | Use `Set result = ...; Quit` pattern then `Return result` after block |
| `New varname` inside method | Illegal — methods are already scoped | Just `Set varname = ""` |
| `Quit value` in Try/Catch | `<QUIT>` error | Use `Return value` |
| `^Temp(key) = value` | Global persists across processes | Use `$$$ISERR` checking or local vars for temp data |
| `$Extract(str, 0)` | 0 returns `""` — IRIS is 1-based | Use `$Extract(str, 1)` |
