Class IadSm173.F37 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Name As %String;

Property Alias As %String;

Property City As %String;

Property Qty As %Integer;

Property Price As %Numeric(SCALE = 2);

Property Placed As %Integer;

Property Stamp As %String;

Property Note As %String(MAXLEN = 255);

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm173F37)</SqlIdExpression>
<SQLMap name="Master">
<Data name="Alias">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="City">
<Delimiter>"^"</Delimiter>
<Piece>3</Piece>
</Data>
<Data name="Name">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="Note">
<Delimiter>"^"</Delimiter>
<Piece>8</Piece>
</Data>
<Data name="Placed">
<Delimiter>"^"</Delimiter>
<Piece>6</Piece>
</Data>
<Data name="Price">
<Delimiter>"^"</Delimiter>
<Piece>5</Piece>
</Data>
<Data name="Qty">
<Delimiter>"^"</Delimiter>
<Piece>4</Piece>
</Data>
<Data name="Stamp">
<Delimiter>"^"</Delimiter>
<Piece>7</Piece>
</Data>
<Global>^IadSm173F37</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<SQLMap name="X">
<Global>^IadSm173F37X</Global>
<Subscript name="1">
<Expression>$$SQLUPPER({Name})</Expression>
</Subscript>
<Subscript name="2">
<Expression>{ID}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<SQLMap name="XA">
<Global>^IadSm173F37A</Global>
<Subscript name="1">
<Expression>$$SQLSTRING({Alias})</Expression>
</Subscript>
<Subscript name="2">
<Expression>{ID}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<SQLMap name="XC">
<Global>^IadSm173F37C</Global>
<Subscript name="1">
<Expression>{City}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{ID}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<StreamLocation>^IadSm173F37S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
