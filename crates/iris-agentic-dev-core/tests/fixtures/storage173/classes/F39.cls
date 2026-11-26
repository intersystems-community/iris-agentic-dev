Class IadSm173.F39 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Name As %String;

Property City As %String;

Property Age As %Integer;

Property Born As %Integer;

Property Status As %String;


Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm173F39)</SqlIdExpression>
<SQLMap name="Master">
<Data name="Age">
<Delimiter>"^"</Delimiter>
<Piece>3</Piece>
</Data>
<Data name="Born">
<Delimiter>"^"</Delimiter>
<Piece>4</Piece>
</Data>
<Data name="City">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="Name">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="Status">
<Delimiter>"^"</Delimiter>
<Piece>5</Piece>
</Data>
<Global>^IadSm173F39</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<SQLMap name="X">
<Global>^IadSm173F39X</Global>
<Subscript name="1">
<Expression>{City}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{Age}</Expression>
</Subscript>
<Subscript name="3">
<Expression>{Status}</Expression>
</Subscript>
<Subscript name="4">
<Expression>{Born}</Expression>
</Subscript>
<Subscript name="5">
<Expression>{Name}</Expression>
</Subscript>
<Subscript name="6">
<Expression>$$SQLUPPER({Name})</Expression>
</Subscript>
<Subscript name="7">
<Expression>$$ALPHAUP({Name})</Expression>
</Subscript>
<Subscript name="8">
<Expression>{ID}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<StreamLocation>^IadSm173F39S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
