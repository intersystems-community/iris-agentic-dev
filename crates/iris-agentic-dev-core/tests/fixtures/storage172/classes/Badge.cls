Class IadSm172.Badge Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Holder As %String;

Property Level As %String;

Index BadgeExtent [ Extent, Type = bitmap ];

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172Badge)</SqlIdExpression>
<SQLMap name="BadgeExtent">
<Global>^IadSm172BadgeX</Global>
<Type>bitmapextent</Type>
</SQLMap>
<SQLMap name="Master">
<Data name="Holder">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="Level">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Global>^IadSm172Badge</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172BadgeS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
