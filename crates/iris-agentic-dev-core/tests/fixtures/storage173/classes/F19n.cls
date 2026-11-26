Class IadSm173.F19n Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Name As %String;

Property City As %String;

Property Age As %Integer;

Property Born As %Integer;

Property Status As %String;

Index MBit On Status [ Type = bitmap ];

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm173F19n)</SqlIdExpression>
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
<Global>^IadSm173F19n</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<SQLMap name="MBit">
<Global>^IadSm173F19nX</Global>
<Subscript name="1">
<Expression>{Status}</Expression>
</Subscript>
<Type>bitmap</Type>
</SQLMap>
<StreamLocation>^IadSm173F19nS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
