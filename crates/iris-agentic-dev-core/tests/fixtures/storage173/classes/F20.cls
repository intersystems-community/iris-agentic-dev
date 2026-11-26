Class IadSm173.F20 Extends %Persistent
{

Property Name As %String;

Property City As %String;

Property Age As %Integer;

Property Born As %Integer;

Property Status As %String;

Index StatusIdx On Status [ Type = bitmap ];
}
