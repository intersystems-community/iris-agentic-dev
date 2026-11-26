Class IadSm173.F38 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Name As %String;

Property City As %String;

Property Age As %Integer;

Property Born As %Integer;

Property Status As %String;


Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm173F38)</SqlIdExpression>
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
<Global>^IadSm173F38</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<SQLMap name="X">
<Global>^IadSm173F38X</Global>
<Subscript name="1">
<Expression>$$SQLUPPER({Name})</Expression>
</Subscript>
<Subscript name="2">
<Expression>{ID}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<SQLMap name="Y">
<Global>^IadSm173F38Y</Global>
<Subscript name="1">
<Expression>{City}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{ID}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<StreamLocation>^IadSm173F38S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
